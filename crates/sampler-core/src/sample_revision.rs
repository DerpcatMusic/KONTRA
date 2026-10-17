//! Immutable source revisions in fixed generation-owned slots. Transactions
//! carry all incoming/retired ownership back to control; audio never drops PCM.
use super::{Candidate, HeadBudget, HeadBudgetStatus, Pcm, Prepared};
use crate::{Error, NotePitch, PlanId, Playback, Runtime};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU8, Ordering},
};

pub const DORMANT_SAMPLE: usize = usize::MAX;
pub const MAX_SAMPLE_REVISIONS: usize = 65_600;
pub const MAX_RETIRED_SAMPLE_REVISIONS: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleTargetId(pub u32);
#[derive(Clone, Debug)]
pub struct SampleTarget {
    pub id: SampleTargetId,
    pub regions: Box<[usize]>,
}
#[derive(Clone, Copy, Debug)]
pub struct SampleRevisionLimits {
    pub slots: usize,
    pub resident_bytes: usize,
}

/// Full immutable backing and source geometry, compiled before publication.
pub struct PreparedSampleRevision {
    pub target: SampleTargetId,
    pub(super) pcm: Pcm,
    cursor: crate::source::Cursor,
    rate: u32,
    bytes: usize,
    _lifetime: Option<Box<dyn Send + Sync>>,
}
impl std::fmt::Debug for PreparedSampleRevision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSampleRevision")
            .field("target", &self.target)
            .field("asset", &self.pcm.asset_id())
            .field("source_rate", &self.pcm.sample_rate())
            .field("frames", &self.pcm.frame_count())
            .finish()
    }
}
impl PreparedSampleRevision {
    /// Control-side preflight before payload reservation or decoding.
    pub fn validate_geometry(frames: usize, source_rate: u32, playback: Playback, output_rate: u32) -> Result<(), Error> {
        if playback.transpose_semitones != 0. || playback.wavetable.is_some() {
            return Err(Error::InvalidInput);
        }
        playback.cursor(frames, source_rate, output_rate).map(|_|())
    }
    /// `playback` describes file/explicit oscillator playback geometry. Pitch
    /// remains the authored region's root/tune; no file root is substituted.
    pub fn new(
        target: SampleTargetId,
        pcm: Pcm,
        playback: Playback,
        output_rate: u32,
    ) -> Result<Self, Error> {
        if playback.transpose_semitones != 0. || playback.wavetable.is_some() {
            return Err(Error::InvalidInput);
        }
        let cursor = playback.cursor(pcm.frame_count(), pcm.sample_rate(), output_rate)?;
        let bytes = pcm.resident_bytes();
        Ok(Self {
            target,
            pcm,
            cursor,
            rate: output_rate,
            bytes,
            _lifetime: None,
        })
    }
    /// Retain a control-owned source registration through all sounding voices.
    pub fn with_lifetime(mut self, lifetime: impl Send + Sync + 'static) -> Self {
        self._lifetime = Some(Box::new(lifetime));
        self
    }
    pub fn asset(&self) -> &Pcm {
        &self.pcm
    }
}

const PENDING: u8 = 0;
const SUCCESS: u8 = 1;
const FAILED: u8 = 2;
const CANCELLED: u8 = 3;
const COMMITTING: u8 = 4;
/// Shared status is published only after source binding commits. Cancellation
/// races with the commit CAS: a winning cancellation cannot mutate the source.
#[derive(Debug)]
pub struct SampleTask {
    state: AtomicU8,
}
impl Default for SampleTask {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(PENDING),
        }
    }
}
impl SampleTask {
    pub fn finished(&self) -> bool {
        matches!(
            self.state.load(Ordering::Acquire),
            SUCCESS | FAILED | CANCELLED
        )
    }
    pub fn success(&self) -> bool {
        self.state.load(Ordering::Acquire) == SUCCESS
    }
    pub fn cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLED
    }
    pub fn cancel(&self) -> bool {
        self.state
            .compare_exchange(PENDING, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn fail(&self) {
        let _ = self
            .state
            .compare_exchange(PENDING, FAILED, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub struct SampleTransaction {
    pub plan: PlanId,
    pub sequence: u64,
    pub task: Arc<SampleTask>,
    pub incoming: Option<Box<PreparedSampleRevision>>,
    /// Every owned revision comes back through the bounded reply ring.
    pub retired: [Option<Box<PreparedSampleRevision>>; MAX_RETIRED_SAMPLE_REVISIONS],
    pub result: Result<(), Error>,
    queued_bytes: usize,
}
impl std::fmt::Debug for SampleTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SampleTransaction")
            .field("plan", &self.plan)
            .field("sequence", &self.sequence)
            .field("incoming", &self.incoming)
            .field("result", &self.result)
            .finish_non_exhaustive()
    }
}
impl SampleTransaction {
    pub fn new(
        plan: PlanId,
        sequence: u64,
        task: Arc<SampleTask>,
        revision: PreparedSampleRevision,
    ) -> Self {
        Self {
            plan,
            sequence,
            task,
            queued_bytes: revision.bytes,
            incoming: Some(Box::new(revision)),
            retired: std::array::from_fn(|_| None),
            result: Err(Error::NotReady),
        }
    }
    pub fn collect(plan: PlanId) -> Self {
        Self {
            plan,
            sequence: 0,
            task: Arc::new(SampleTask::default()),
            incoming: None,
            queued_bytes: 0,
            retired: std::array::from_fn(|_| None),
            result: Err(Error::NotReady),
        }
    }
}

struct Slot {
    revision: Option<Box<PreparedSampleRevision>>,
    voices: usize,
    bindings: usize,
}
pub(crate) struct SampleState {
    slots: Box<[Slot]>,
    bindings: Box<[Option<usize>]>,
    sequences: Box<[u64]>,
    bytes: usize,
    budget: usize,
}
impl SampleState {
    pub fn new(prepared: &Prepared) -> Self {
        let Some(limits) = prepared.sample_limits else {
            return Self {
                slots: Box::new([]),
                bindings: Box::new([]),
                sequences: Box::new([]),
                bytes: 0,
                budget: 0,
            };
        };
        Self {
            slots: (0..limits.slots)
                .map(|_| Slot {
                    revision: None,
                    voices: 0,
                    bindings: 0,
                })
                .collect(),
            bindings: vec![None; prepared.regions.len()].into(),
            sequences: vec![0; prepared.sample_targets.len()].into(),
            bytes: 0,
            budget: limits.resident_bytes,
        }
    }
    pub fn resident_bytes(&self) -> usize {
        self.bytes
    }
    pub fn source_storage<'a>(&'a self, prepared: &'a Prepared, sample: usize)
        -> Option<crate::SourceStorageRef<'a>> {
        if sample < prepared.sample_count() { prepared.source_storage(sample) }
        else { self.slots.get(sample.checked_sub(prepared.sample_count())?)?.revision.as_ref()
            .map(|revision| crate::SourceStorageRef::Stereo(&revision.pcm)) }
    }
    pub fn asset<'a>(&'a self, prepared: &'a Prepared, sample: usize) -> Option<&'a Pcm> {
        if sample < prepared.sample_count() {
            prepared.source_storage(sample).and_then(crate::SourceStorageRef::stereo)
        } else {
            self.slots
                .get(sample.checked_sub(prepared.sample_count())?)?
                .revision
                .as_ref()
                .map(|r| &r.pcm)
        }
    }
    pub fn pin(&mut self, prepared: &Prepared, sample: usize) -> Result<(), Error> {
        if sample < prepared.sample_count() {
            return Ok(());
        }
        let slot = self
            .slots
            .get_mut(sample - prepared.sample_count())
            .ok_or(Error::InvalidInput)?;
        if slot.revision.is_none() {
            return Err(Error::StaleHandle);
        }
        slot.voices = slot.voices.checked_add(1).ok_or(Error::Capacity)?;
        Ok(())
    }
    pub fn unpin(&mut self, prepared: &Prepared, sample: usize) {
        if sample >= prepared.sample_count() {
            let slot = &mut self.slots[sample - prepared.sample_count()];
            debug_assert!(slot.voices > 0);
            slot.voices -= 1;
        }
    }
    pub(super) fn source(
        &self,
        prepared: &Prepared,
        candidate: Candidate,
        pitch: NotePitch,
    ) -> Option<(usize, crate::source::Cursor)> {
        let region = &prepared.regions[candidate.region];
        if let Some(slot) = self.bindings.get(candidate.region).copied().flatten() {
            let revision = self.slots[slot].revision.as_ref()?;
            let ratio = if region.sample == DORMANT_SAMPLE {
                f64::from(revision.pcm.sample_rate()) / f64::from(prepared.rate)
            } else {
                f64::from(revision.pcm.sample_rate())
                    / f64::from(prepared.source_storage(region.sample)?.sample_rate())
            };
            let step = prepared.step(candidate, pitch) * ratio;
            if !crate::lower::PITCH_STEPS.contains(&step) {
                return None;
            }
            Some((prepared.sample_count() + slot, revision.cursor.with_step(step)))
        } else {
            let cursor = region
                .cursor?
                .cursor(&prepared.cursor_loops)
                .with_step(prepared.step(candidate, pitch));
            Some((region.sample, cursor))
        }
    }
    pub(super) fn rejection(
        &self,
        prepared: &Prepared,
        candidate: Candidate,
        pitch: NotePitch,
    ) -> Option<crate::Rejection> {
        if self.source(prepared, candidate, pitch).is_some() {
            return None;
        }
        if self
            .bindings
            .get(candidate.region)
            .copied()
            .flatten()
            .is_some()
        {
            Some(crate::Rejection::SourcePitch)
        } else {
            Some(crate::Rejection::SourceUnavailable)
        }
    }
    fn collect(&mut self, transaction: &mut SampleTransaction) {
        let mut next = 0;
        for slot in &mut self.slots {
            if slot.voices == 0 && slot.bindings == 0 && slot.revision.is_some() {
                while next < MAX_RETIRED_SAMPLE_REVISIONS && transaction.retired[next].is_some() {
                    next += 1;
                }
                if next == MAX_RETIRED_SAMPLE_REVISIONS {
                    break;
                }
                let retired = slot.revision.take().unwrap();
                self.bytes -= retired.bytes;
                transaction.retired[next] = Some(retired);
                next += 1;
            }
        }
    }
    fn commit(
        &mut self,
        prepared: &Prepared,
        transaction: &mut SampleTransaction,
    ) -> Result<(), Error> {
        let Some(incoming) = transaction.incoming.as_ref() else {
            self.collect(transaction);
            return Ok(());
        };
        if incoming.rate != prepared.rate || transaction.sequence == 0 {
            return Err(Error::InvalidInput);
        }
        let target = prepared
            .sample_targets
            .binary_search_by_key(&incoming.target, |t| t.id)
            .map_err(|_| Error::InvalidInput)?;
        if transaction.sequence <= self.sequences[target] {
            return Err(Error::StaleHandle);
        }
        let mapping = &prepared.sample_targets[target];
        // This queue publishes exactly one two-lane Pcm. A logical original-
        // channel target remains queryable, but cannot change source kind here.
        // Defend the actual commit before slots, bindings, or the task CAS.
        if !prepared.sample_target_accepts_pcm(target) {
            return Err(Error::UnsupportedSourceLayout);
        }
        for &index in &mapping.regions {
            let r = &prepared.regions[index];
            if r.cursor
                .is_some_and(|c| c.cursor(&prepared.cursor_loops).wavetable().is_some())
            {
                return Err(Error::InvalidInput);
            }
        }
        // Prefer replacing a target's no-voice revision in place. Every other
        // sounding or future-bound revision keeps its slot and exact metadata.
        let reuse = self.bindings[mapping.regions[0]].filter(|&index| {
            let slot = &self.slots[index];
            slot.voices == 0
                && slot.bindings == mapping.regions.len()
                && mapping
                    .regions
                    .iter()
                    .all(|&r| self.bindings[r] == Some(index))
        });
        let free = reuse
            .or_else(|| {
                self.slots.iter().position(|slot| {
                    slot.revision.is_none() || slot.voices == 0 && slot.bindings == 0
                })
            })
            .ok_or(Error::Capacity)?;
        let retired_bytes = self.slots[free].revision.as_ref().map_or(0, |r| r.bytes);
        let bytes = self
            .bytes
            .checked_sub(retired_bytes)
            .and_then(|b| b.checked_add(incoming.bytes))
            .ok_or(Error::Capacity)?;
        if bytes > self.budget || transaction.retired[0].is_some() {
            return Err(Error::Capacity);
        }
        transaction
            .task
            .state
            .compare_exchange(PENDING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleHandle)?;
        for &region in &mapping.regions {
            if let Some(old) = self.bindings[region].replace(free) {
                self.slots[old].bindings -= 1;
            }
        }
        transaction.retired[0] = self.slots[free].revision.take();
        self.slots[free].revision = transaction.incoming.take();
        self.slots[free].bindings = mapping.regions.len();
        self.bytes = bytes;
        self.sequences[target] = transaction.sequence;
        self.collect(transaction);
        Ok(())
    }
}

impl Prepared {
    fn sample_target_accepts_pcm(&self, index: usize) -> bool {
        self.sample_targets[index].regions.iter().all(|&region|
            self.region_source_storage(region).is_none_or(|source| source.stereo().is_some()))
    }
    /// Mapping and all queue/slot allocations happen before audio construction.
    /// Targets retain their logical source identity, including channel families;
    /// the single-Pcm update API admits only dormant or stereo source targets.
    pub fn with_sample_targets(
        mut self,
        mut targets: Vec<SampleTarget>,
        limits: SampleRevisionLimits,
    ) -> Result<Self, Error> {
        if self.zone_routing.is_some() {
            return Err(Error::UnsupportedZoneRoutingSampleUpdates);
        }
        if limits.slots == 0
            || limits.slots > MAX_SAMPLE_REVISIONS
            || limits.resident_bytes == 0
            || targets.is_empty()
            || targets.len() > 65536
        {
            return Err(Error::InvalidInput);
        }
        targets.sort_by_key(|t| t.id);
        let mut seen = std::collections::HashSet::new();
        for (i, t) in targets.iter().enumerate() {
            if t.regions.is_empty() || i > 0 && targets[i - 1].id == t.id {
                return Err(Error::InvalidInput);
            }
            for &r in &t.regions {
                if r >= self.regions.len() || !seen.insert(r) {
                    return Err(Error::InvalidInput);
                }
            }
        }
        self.sample_targets = targets.into();
        self.sample_limits = Some(limits);
        Ok(self)
    }
}

/// One script/control producer, never shared by audio. Every accepted payload
/// returns intact for control-side reuse/destruction, even on explicit failure.
pub struct SampleControl {
    pending: Producer<SampleTransaction>,
    replies: Consumer<SampleTransaction>,
    plan: PlanId,
    sequence: u64,
    queued_bytes: usize,
    byte_limit: usize,
    // Sorted immutable capabilities of the exact producer PlanId. No callback
    // lookup/allocation or implicit first-pack source substitution.
    targets: Box<[(SampleTargetId, bool)]>,
    head_budget: HeadBudget,
    wake: Arc<OnceLock<std::thread::Thread>>,
}
pub(crate) struct SampleQueues {
    pending: Consumer<SampleTransaction>,
    replies: Producer<SampleTransaction>,
    retained: Option<SampleTransaction>,
    head_budget: HeadBudget,
    wake: Arc<OnceLock<std::thread::Thread>>,
}
impl SampleControl {
    /// Off audio: physical dynamic PCM quota, shared by packed heads and full
    /// resident buffers through staging/current/old voices/retired replies.
    /// Initial static Streamer storage is separate.
    pub fn physical_budget(&self)->HeadBudget {self.head_budget.clone()}
    /// Compatibility name; now also includes budgeted full-resident PCM.
    pub fn head_budget(&self)->HeadBudget {self.physical_budget()}
    /// Off audio: wake the sole owner when a commit/failure receipt arrives.
    pub fn set_wake(&mut self, owner: std::thread::Thread) -> Result<(), Error> {
        self.wake.set(owner).map_err(|_| Error::InvalidInput)
    }
    pub fn plan(&self) -> PlanId {
        self.plan
    }
    /// Off audio, before source preparation: validate this producer's target and
    /// whether its one-Pcm publication preserves the compiled source layout.
    pub fn validate_target(&self, target: SampleTargetId) -> Result<(), Error> {
        let index = self.targets.binary_search_by_key(&target, |entry| entry.0)
            .map_err(|_| Error::InvalidInput)?;
        if !self.targets[index].1 { return Err(Error::UnsupportedSourceLayout); }
        Ok(())
    }
    #[allow(clippy::result_large_err)]
    pub fn submit(
        &mut self,
        revision: PreparedSampleRevision,
        task: Arc<SampleTask>,
    ) -> Result<u64, (Error, PreparedSampleRevision, Arc<SampleTask>)> {
        if let Err(error) = self.validate_target(revision.target) {
            return Err((error, revision, task));
        }
        if revision.bytes > self.byte_limit {
            return Err((Error::InvalidInput, revision, task));
        }
        if revision.pcm.0.full {
            if let Err(error)=revision.pcm.bind_resident_budget(&self.head_budget) {return Err((error,revision,task));}
        } else if !revision.pcm.0.head_budget.get().is_some_and(|budget|budget.same(&self.head_budget)) {
            return Err((Error::RevisionConflict,revision,task));
        }
        if self.pending.is_abandoned() {
            return Err((Error::NotReady, revision, task));
        }
        if self.sequence == u64::MAX
            || self.pending.is_full()
            || self
                .queued_bytes
                .checked_add(revision.bytes)
                .is_none_or(|b| b > self.byte_limit)
        {
            return Err((Error::Capacity, revision, task));
        }
        let sequence = self.sequence + 1;
        let bytes = revision.bytes;
        let transaction = SampleTransaction::new(self.plan, sequence, task, revision);
        match self.pending.push(transaction) {
            Ok(()) => {
                self.sequence = sequence;
                self.queued_bytes += bytes;
                Ok(sequence)
            }
            Err(PushError::Full(mut t)) => {
                Err((Error::Capacity, *t.incoming.take().unwrap(), t.task))
            }
        }
    }
    pub fn collect(&mut self) -> Result<(), Error> {
        self.pending
            .push(SampleTransaction::collect(self.plan))
            .map_err(|_| Error::Capacity)
    }
    pub fn reply(&mut self) -> Option<SampleTransaction> {
        let transaction = self.replies.pop().ok()?;
        self.queued_bytes -= transaction.queued_bytes;
        if transaction.result.is_ok() {
            transaction.task.state.store(SUCCESS, Ordering::Release);
        }
        Some(transaction)
    }
}
impl Runtime {
    /// Bounded atomic accounting for dynamic packed heads and full-resident
    /// PCM. Includes staging and returned retired payloads, not only slots.
    pub fn sample_physical_budget_status(&self)->Option<HeadBudgetStatus> {
        self.sample_queues.as_ref().map(|q|q.head_budget.status())
    }
    /// Compatibility name; the same quota also covers full-resident PCM.
    pub fn sample_head_budget_status(&self)->Option<HeadBudgetStatus> {
        self.sample_queues.as_ref().map(|q|q.head_budget.status())
    }
    /// Control/readback borrow of the actual committed source, including an
    /// initially dormant target. Its lifetime cannot outlive this runtime borrow.
    pub fn sample_target_asset(&self, target: SampleTargetId) -> Result<&Pcm, Error> {
        self.sample_target_source_storage(target)?.stereo().ok_or(Error::UnsupportedSourceLayout)
    }
    /// Source-kind-aware committed readback. A family never aliases its first pack.
    pub fn sample_target_source_storage(&self, target: SampleTargetId)
        -> Result<crate::SourceStorageRef<'_>, Error> {
        let generation = self
            .plans
            .get(self.active_plan.0)
            .ok_or(Error::StaleHandle)?;
        let prepared = &generation.prepared;
        let index = prepared
            .sample_targets
            .binary_search_by_key(&target, |t| t.id)
            .map_err(|_| Error::InvalidInput)?;
        let region = prepared.sample_targets[index].regions[0];
        let sample = generation.samples.bindings[region]
            .map_or(prepared.regions[region].sample, |s| prepared.sample_count() + s);
        generation
            .samples
            .source_storage(prepared, sample)
            .ok_or(Error::NotReady)
    }
    pub fn sample_target_count(&self) -> usize {
        self.plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .sample_targets
            .len()
    }
    /// Off audio, before installing the producer on the script's owner thread.
    pub fn with_sample_updates(mut self, queued: usize) -> Result<(Self, SampleControl), Error> {
        if queued == 0
            || queued > 64
            || self.sample_queues.is_some()
            || self
                .plans
                .get(self.active_plan.0)
                .unwrap()
                .prepared
                .sample_limits
                .is_none()
        {
            return Err(Error::InvalidInput);
        }
        let (pending, incoming) = RingBuffer::new(queued);
        let (replies, outgoing) = RingBuffer::new(queued);
        let wake = Arc::new(OnceLock::new());
        let plan = self.active_plan;
        let byte_limit = self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .sample_limits
            .unwrap()
            .resident_bytes;
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let targets = prepared.sample_targets.iter().enumerate()
            .map(|(index, target)| (target.id, prepared.sample_target_accepts_pcm(index)))
            .collect();
        let head_budget=HeadBudget::new(byte_limit);
        self.sample_queues = Some(SampleQueues {
            wake: wake.clone(),
            pending: incoming,
            replies,
            retained: None,
            head_budget: head_budget.clone(),
        });
        Ok((
            self,
            SampleControl {
                pending,
                replies: outgoing,
                plan,
                sequence: 0,
                queued_bytes: 0,
                byte_limit,
                targets,
                head_budget,
                wake,
            },
        ))
    }
    /// One bounded transaction at a render boundary, without destructor work.
    pub fn poll_sample_update(&mut self) -> Result<bool, Error> {
        let Some(queues) = self.sample_queues.as_mut() else {
            return Ok(false);
        };
        if queues.replies.is_abandoned() || queues.replies.is_full() {
            return Err(Error::Capacity);
        }
        if let Some(transaction) = queues.retained.take() {
            if let Err(PushError::Full(transaction)) = queues.replies.push(transaction) {
                queues.retained = Some(transaction);
            }
            return Ok(true);
        }
        let Ok(mut transaction) = queues.pending.pop() else {
            return Ok(false);
        };
        let result = if transaction.plan != self.active_plan {
            Err(Error::StaleHandle)
        } else {
            match self.plans.get_mut(transaction.plan.0) {
                Some(generation) => generation
                    .samples
                    .commit(&generation.prepared, &mut transaction),
                None => Err(Error::StaleHandle),
            }
        };
        transaction.result = result;
        if result.is_err() {
            transaction.task.fail();
        }
        let queues = self.sample_queues.as_mut().unwrap();
        if let Err(PushError::Full(transaction)) = queues.replies.push(transaction) {
            queues.retained = Some(transaction);
        }
        if let Some(owner) = queues.wake.get() {
            owner.unpark();
        }
        Ok(true)
    }
}
