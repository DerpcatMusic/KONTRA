//! Bounded decoded-page ownership. One audio cache and one worker coordinator.
use crate::{AssetId, Error, Frame, Pcm};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::{
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

pub const PAGE_FRAMES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageKey {
    pub asset: AssetId,
    pub index: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeFailure {
    Unavailable,
    InvalidSamples,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageStatus {
    Missing,
    Pending,
    Ready,
    Failed(DecodeFailure),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    NotConfigured,
    ClockOverflow,
    InvalidRange,
    Capacity,
    Disconnected,
    SequenceExhausted,
    WrongWorker,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageUpdate {
    Loaded(PageKey),
    Failed(PageKey, DecodeFailure),
    Discarded(PageKey),
}

#[derive(Clone, Copy, Debug)]
struct Request {
    key: PageKey,
    len: usize,
    deadline: u64,
    slot: usize,
    serial: u64,
    store: u64,
}

/// A worker-owned decode buffer. Fill every exposed frame, then complete or fail it.
/// Dropping a job is allowed on control/worker but permanently removes its buffer
/// from this pool. No job or endpoint may be destroyed on the audio thread.
pub struct DecodeJob {
    request: Request,
    samples: Box<[Frame]>,
}
impl DecodeJob {
    pub fn key(&self) -> PageKey {
        self.request.key
    }
    pub fn range(&self) -> Range<usize> {
        let start = self.request.key.index * PAGE_FRAMES;
        start..start + self.request.len
    }
    pub fn deadline(&self) -> u64 {
        self.request.deadline
    }
    pub fn frames_mut(&mut self) -> &mut [Frame] {
        &mut self.samples[..self.request.len]
    }
}
impl std::fmt::Debug for DecodeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodeJob")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}
#[derive(Debug)]
pub struct RejectedDecode {
    pub reason: StreamError,
    pub job: DecodeJob,
    pub result: Result<(), DecodeFailure>,
}
struct Completed {
    job: DecodeJob,
    result: Result<(), DecodeFailure>,
}
enum State {
    Pending,
    Ready(Box<[Frame]>),
    Failed(DecodeFailure),
}
struct Entry {
    request: Request,
    used: u64,
    state: State,
}
impl Entry {
    fn status(&self) -> PageStatus {
        match &self.state {
            State::Pending => PageStatus::Pending,
            State::Ready(_) => PageStatus::Ready,
            State::Failed(error) => PageStatus::Failed(*error),
        }
    }
}

/// Audio-owned page slots. Construction/destruction belong off audio. Protect all
/// current demand before requesting replacements in an epoch, so voice order
/// cannot evict another voice's required page. A cache miss never waits.
pub struct StreamCache {
    entries: Box<[Option<Entry>]>,
    index: Vec<(PageKey, usize)>,
    requests: Producer<Request>,
    completed: Consumer<Completed>,
    recycled: Producer<Box<[Frame]>>,
    epoch: u64,
    serial: u64,
    store: u64,
}
/// One coordinator serializes page requests/results around any worker executors.
/// Jobs own buffers and can move to workers; this endpoint remains a single writer.
pub struct StreamWorker {
    requests: Consumer<Request>,
    completed: Producer<Completed>,
    recycled: Consumer<Box<[Frame]>>,
    pending: Box<[Option<Request>]>,
    launched: Box<[u64]>,
    free: Vec<Box<[Frame]>>,
    store: u64,
}
impl StreamCache {
    /// Bytes of page buffers this cache owns once its worker has filled them.
    pub fn bytes(&self) -> usize {
        self.entries.len() * PAGE_FRAMES * size_of::<Frame>()
    }
    pub fn new(pages: usize) -> Result<(Self, StreamWorker), Error> {
        if pages == 0 {
            return Err(Error::InvalidInput);
        }
        std::alloc::Layout::array::<Option<Entry>>(pages).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<Completed>(pages).map_err(|_| Error::Capacity)?;
        pages
            .checked_mul(PAGE_FRAMES)
            .and_then(|n| n.checked_mul(std::mem::size_of::<Frame>()))
            .ok_or(Error::Capacity)?;
        let mut free = Vec::new();
        free.try_reserve_exact(pages).map_err(|_| Error::Capacity)?;
        for _ in 0..pages {
            let mut frames = Vec::new();
            frames
                .try_reserve_exact(PAGE_FRAMES)
                .map_err(|_| Error::Capacity)?;
            frames.resize(PAGE_FRAMES, [0.; 2]);
            free.push(frames.into_boxed_slice());
        }
        static NEXT_STORE: AtomicU64 = AtomicU64::new(1);
        #[allow(deprecated, reason = "fetch_update supports the Rust 1.92 minimum")]
        let store = NEXT_STORE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity)?;
        let (requests, incoming) = RingBuffer::new(pages);
        let (outgoing, completed) = RingBuffer::new(pages);
        let (recycled, returned) = RingBuffer::new(pages);
        Ok((
            Self {
                entries: (0..pages).map(|_| None).collect(),
                index: Vec::with_capacity(pages),
                requests,
                completed,
                recycled,
                epoch: 1,
                serial: 0,
                store,
            },
            StreamWorker {
                requests: incoming,
                completed: outgoing,
                recycled: returned,
                pending: vec![None; pages].into_boxed_slice(),
                launched: vec![0; pages].into_boxed_slice(),
                free,
                store,
            },
        ))
    }
    pub fn begin_epoch(&mut self) -> Result<(), StreamError> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or(StreamError::SequenceExhausted)?;
        Ok(())
    }
    fn find(&self, key: PageKey) -> Option<usize> {
        self.index
            .binary_search_by_key(&key, |(key, _)| *key)
            .ok()
            .map(|i| self.index[i].1)
    }
    pub fn status(&self, key: PageKey) -> PageStatus {
        self.find(key).map_or(PageStatus::Missing, |i| {
            self.entries[i].as_ref().unwrap().status()
        })
    }
    /// Protect resident and in-flight pages for this epoch. Returns complete
    /// readiness separately from protection; absent/failed pages are not ready.
    pub fn protect(&mut self, asset: &Pcm, frames: Range<usize>) -> Result<bool, StreamError> {
        if frames.start >= frames.end || frames.end > asset.frame_count() {
            return Err(StreamError::InvalidRange);
        }
        let first = frames.start / PAGE_FRAMES;
        let last = (frames.end - 1) / PAGE_FRAMES;
        let mut ready = 0;
        let begin = self.index.partition_point(|(key, _)| {
            *key < PageKey {
                asset: asset.asset_id(),
                index: first,
            }
        });
        for &(key, slot) in &self.index[begin..] {
            if key.asset != asset.asset_id() || key.index > last {
                break;
            }
            let entry = self.entries[slot].as_mut().unwrap();
            entry.used = self.epoch;
            ready += usize::from(entry.status() == PageStatus::Ready);
        }
        Ok(ready == last - first + 1)
    }
    /// Request one validated decoded page. Hits share storage across notes/views.
    /// A failed page remains failed until evicted or a new asset revision is used.
    pub fn request(
        &mut self,
        asset: &Pcm,
        page: usize,
        deadline: u64,
    ) -> Result<PageStatus, StreamError> {
        let start = page
            .checked_mul(PAGE_FRAMES)
            .filter(|start| *start < asset.frame_count())
            .ok_or(StreamError::InvalidRange)?;
        let key = PageKey {
            asset: asset.asset_id(),
            index: page,
        };
        if let Some(slot) = self.find(key) {
            let entry = self.entries[slot].as_mut().unwrap();
            if matches!(entry.state, State::Pending) && self.requests.is_abandoned() {
                return Err(StreamError::Disconnected);
            }
            entry.used = self.epoch;
            if matches!(entry.state, State::Pending) && deadline < entry.request.deadline {
                let request = Request {
                    deadline,
                    ..entry.request
                };
                self.requests
                    .push(request)
                    .map_err(|_| StreamError::Capacity)?;
                entry.request = request;
            }
            return Ok(entry.status());
        }
        if self.requests.is_abandoned() {
            return Err(StreamError::Disconnected);
        }
        if self.requests.is_full() {
            return Err(StreamError::Capacity);
        }
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .filter_map(|(i, entry)| {
                        entry
                            .as_ref()
                            .filter(|e| e.used != self.epoch)
                            .map(|e| (i, e.used))
                    })
                    .min_by_key(|(_, used)| *used)
                    .map(|(i, _)| i)
            })
            .ok_or(StreamError::Capacity)?;
        if self.recycled.is_full()
            && self.entries[slot]
                .as_ref()
                .is_some_and(|e| matches!(e.state, State::Ready(_)))
        {
            return Err(StreamError::Capacity);
        }
        let serial = self
            .serial
            .checked_add(1)
            .ok_or(StreamError::SequenceExhausted)?;
        let request = Request {
            key,
            len: PAGE_FRAMES.min(asset.frame_count() - start),
            deadline,
            slot,
            serial,
            store: self.store,
        };
        // Single producer, and the consumer only frees space after the check.
        self.requests
            .push(request)
            .expect("reserved request capacity");
        self.serial = serial;
        if let Some(old) = self.entries[slot].take() {
            let index = self
                .index
                .binary_search_by_key(&old.request.key, |(key, _)| *key)
                .unwrap();
            self.index.remove(index);
            if let State::Ready(samples) = old.state {
                self.recycled
                    .push(samples)
                    .expect("reserved return capacity");
            }
        }
        self.entries[slot] = Some(Entry {
            request,
            used: self.epoch,
            state: State::Pending,
        });
        let index = self.index.partition_point(|(found, _)| *found < key);
        self.index.insert(index, (key, slot));
        Ok(PageStatus::Pending)
    }
    /// Explicitly invalidate one page, including a failed request before retry.
    /// An in-flight result becomes stale; ready storage returns to the worker.
    pub fn invalidate(&mut self, key: PageKey) -> Result<bool, StreamError> {
        let Some(slot) = self.find(key) else {
            return Ok(false);
        };
        if self.recycled.is_full()
            && self.entries[slot]
                .as_ref()
                .is_some_and(|e| matches!(e.state, State::Ready(_)))
        {
            return Err(StreamError::Capacity);
        }
        let entry = self.entries[slot].take().unwrap();
        if let State::Ready(samples) = entry.state {
            self.recycled
                .push(samples)
                .expect("reserved return capacity");
        }
        let index = self
            .index
            .binary_search_by_key(&key, |(key, _)| *key)
            .unwrap();
        self.index.remove(index);
        Ok(true)
    }

    /// Admit at most one completion. Stale results return their buffer to the
    /// worker; they cannot replace a reused slot or destroy storage on audio.
    pub fn poll(&mut self) -> Option<PageUpdate> {
        if self.recycled.is_full() {
            return None;
        }
        let completed = self.completed.pop().ok()?;
        let DecodeJob { request, samples } = completed.job;
        let entry = &mut self.entries[request.slot];
        if !entry
            .as_ref()
            .is_some_and(|e| e.request.serial == request.serial && e.request.key == request.key)
        {
            self.recycled
                .push(samples)
                .expect("reserved return capacity");
            return Some(PageUpdate::Discarded(request.key));
        }
        let entry = entry.as_mut().unwrap();
        match completed.result {
            Ok(()) => {
                entry.state = State::Ready(samples);
                Some(PageUpdate::Loaded(request.key))
            }
            Err(error) => {
                self.recycled
                    .push(samples)
                    .expect("reserved return capacity");
                entry.state = State::Failed(error);
                Some(PageUpdate::Failed(request.key, error))
            }
        }
    }
    pub fn frame(&self, asset: AssetId, frame: usize) -> Option<Frame> {
        self.span(asset, frame..frame.checked_add(1)?).map(|s| s[0])
    }
    /// Borrow only an entirely resident, contiguous range within one page.
    pub fn span(&self, asset: AssetId, frames: Range<usize>) -> Option<&[Frame]> {
        if frames.start > frames.end {
            return None;
        }
        if frames.is_empty() {
            return Some(&[]);
        }
        let page = frames.start / PAGE_FRAMES;
        if (frames.end - 1) / PAGE_FRAMES != page {
            return None;
        }
        let entry = self.entries[self.find(PageKey { asset, index: page })?].as_ref()?;
        let start = frames.start % PAGE_FRAMES;
        let end = start.checked_add(frames.len())?;
        if end > entry.request.len {
            return None;
        }
        match &entry.state {
            State::Ready(samples) => Some(&samples[start..end]),
            _ => None,
        }
    }
}
impl StreamWorker {
    /// Recycle returned buffers, coalesce superseded slot requests, then choose
    /// the earliest deadline. All worker storage is bounded by cache capacity.
    pub fn next_job(&mut self) -> Option<DecodeJob> {
        while let Ok(samples) = self.recycled.pop() {
            self.free.push(samples);
        }
        while let Ok(request) = self.requests.pop() {
            let slot = request.slot;
            if request.serial > self.launched[slot]
                && self.pending[slot].is_none_or(|old| old.serial <= request.serial)
            {
                self.pending[slot] = Some(request);
            }
        }
        if self.requests.is_abandoned() || self.free.is_empty() {
            return None;
        }
        let slot = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(i, request)| request.map(|r| (i, r.deadline)))
            .min_by_key(|(_, deadline)| *deadline)?
            .0;
        let request = self.pending[slot].take().unwrap();
        self.launched[slot] = request.serial;
        let mut samples = self.free.pop().unwrap();
        samples[..request.len].fill([0.; 2]);
        Some(DecodeJob { request, samples })
    }
    /// Validate the decoded page off audio. Rejection retains ownership for retry;
    /// failed/invalid decodes publish a visible failure and recycle their buffer.
    pub fn complete(
        &mut self,
        job: DecodeJob,
        result: Result<(), DecodeFailure>,
    ) -> Result<(), RejectedDecode> {
        if job.request.store != self.store {
            return Err(RejectedDecode {
                reason: StreamError::WrongWorker,
                job,
                result,
            });
        }
        if self.completed.is_abandoned() {
            return Err(RejectedDecode {
                reason: StreamError::Disconnected,
                job,
                result,
            });
        }
        let result = if result.is_ok()
            && job.samples[..job.request.len]
                .iter()
                .flatten()
                .any(|v| !v.is_finite())
        {
            Err(DecodeFailure::InvalidSamples)
        } else {
            result
        };
        self.completed
            .push(Completed { job, result })
            .map_err(|PushError::Full(done)| RejectedDecode {
                reason: StreamError::Capacity,
                job: done.job,
                result: done.result,
            })
    }
}

impl crate::Runtime {
    /// Setup/control-side ownership transfer. Construct and destroy runtime/cache
    /// on control; the audio path only moves page buffers through bounded queues.
    pub fn with_stream_cache(mut self, cache: StreamCache) -> Self {
        self.stream_cache = Some(cache);
        self
    }
    /// Audio-side demand/poll access. Protect current demand before requesting
    /// replacement pages; never invalidate pages still needed by playing voices.
    pub fn stream_cache_mut(&mut self) -> Option<&mut StreamCache> {
        self.stream_cache.as_mut()
    }
    pub fn stream_underruns(&self) -> u64 {
        self.stream_underruns
    }

    /// Poll a bounded batch, protect every live source's horizon, then request its
    /// pages with first-use deadlines. No voice/clock advancement or worker waiting.
    /// True means the complete snapshot horizon is resident; false includes pending
    /// or failed pages (inspect page status and explicitly invalidate failures).
    /// A queue/cache error leaves accepted requests intact and reports incomplete
    /// service. Requery after events. Cold onsets require control-side preloading.
    pub fn service_streaming(&mut self, frames: u32) -> Result<bool, StreamError> {
        self.now
            .checked_add(u64::from(frames))
            .ok_or(StreamError::ClockOverflow)?;
        let mut cache = self.stream_cache.take().ok_or(StreamError::NotConfigured)?;
        // Temporarily detach only the audio-owned cache to borrow the immutable
        // voice/plan snapshot. The visitor cannot execute callbacks or mutate it.
        let result = self.service_cache(&mut cache, frames);
        self.stream_cache = Some(cache);
        result
    }

    fn service_cache(&self, cache: &mut StreamCache, frames: u32) -> Result<bool, StreamError> {
        for _ in 0..cache.entries.len() {
            if cache.poll().is_none() {
                break;
            }
        }
        cache.begin_epoch()?;
        let mut ready = true;
        // Protect all voices before any eviction: admission order must never evict
        // a page that a later voice already needs in this same snapshot horizon.
        for requesting in [false, true] {
            for (index, slot) in self.voices.slots.iter().enumerate() {
                let Some(voice) = &slot.value else { continue };
                let family = self.families.get(voice.family.0).unwrap();
                let note = self.notes.get(family.note.0).unwrap();
                let asset = &self.plans.get(note.plan.0).unwrap().prepared.pcm[voice.sample];
                if asset.resident_frames().is_some() {
                    continue;
                }
                // A busy lock reads as nothing resident: pages are merely requested.
                let head = asset.try_head();
                let head = head.as_deref().map_or(&[][..], |h| h);
                let mut failure = None;
                // Visit each page once per run of demand on it, at its first
                // (earliest) deadline.
                let mut last = None;
                let mut visit = |frames: std::ops::Range<usize>, deadline: u64| {
                    crate::prepare::uncovered(head, frames, |frames| {
                        for page in frames.start / PAGE_FRAMES..=(frames.end - 1) / PAGE_FRAMES {
                            if last.replace(page) == Some(page) {
                                continue;
                            }
                            if !requesting {
                                let start = page * PAGE_FRAMES;
                                let end = (start + PAGE_FRAMES).min(asset.frame_count());
                                cache
                                    .protect(asset, start..end)
                                    .expect("validated source demand");
                                continue;
                            }
                            match cache.request(asset, page, deadline) {
                                Ok(status) => ready &= status == PageStatus::Ready,
                                Err(error) => {
                                    failure = Some(error);
                                    return false;
                                }
                            }
                        }
                        true
                    })
                };
                let Some(demand) = self
                    .voice_demand(crate::VoiceId(self.voices.id(index)), frames)
                    .expect("live voice and validated horizon")
                else {
                    continue;
                };
                let cursor = demand.cursor;
                if let Some((reach, direction, lead)) = cursor.linear_reach(demand.frames) {
                    // A plain stretch: whole pages in traversal order, without
                    // walking every output frame.
                    let deadline =
                        |index| demand.at + u64::from(cursor.first_use(index, direction, lead));
                    let mut part = match direction {
                        crate::Direction::Forward => reach.start..reach.start,
                        crate::Direction::Reverse => reach.end..reach.end,
                    };
                    loop {
                        let (frames, first) = match direction {
                            crate::Direction::Forward if part.end < reach.end => {
                                let end =
                                    ((part.end / PAGE_FRAMES + 1) * PAGE_FRAMES).min(reach.end);
                                part = part.end..end;
                                (part.clone(), part.start)
                            }
                            crate::Direction::Reverse if part.start > reach.start => {
                                let start =
                                    ((part.start - 1) / PAGE_FRAMES * PAGE_FRAMES).max(reach.start);
                                part = start..part.start;
                                (part.clone(), part.end - 1)
                            }
                            _ => break,
                        };
                        if !visit(frames, deadline(first)) {
                            break;
                        }
                    }
                } else if let Some(reach) = cursor.loop_reach(demand.frames) {
                    // A loop: its few ranges, each at its first deadline.
                    for (range, at) in reach.into_iter().flatten() {
                        if !visit(range, demand.at + u64::from(at)) {
                            break;
                        }
                    }
                } else {
                    let complete =
                        cursor.visit_demand(demand.frames, demand.envelope, |offset, frames| {
                            visit(frames, demand.at + u64::from(offset))
                        });
                    debug_assert!(complete || failure.is_some());
                }
                if let Some(error) = failure {
                    return Err(error);
                }
            }
        }
        Ok(ready)
    }

    pub(crate) fn check_source_ready(
        &self,
        asset: &Pcm,
        cursor: crate::source::Cursor,
        envelope: crate::Envelope,
    ) -> Result<(), Error> {
        if asset.resident_frames().is_some() {
            return Ok(());
        }
        let cache = self.stream_cache.as_ref().ok_or(Error::NotReady)?;
        let head = asset.try_head();
        let head = head.as_deref().map_or(&[][..], |h| h);
        let ready = cursor.visit_demand(1, crate::EnvelopeState::new(envelope), |_, range| {
            crate::prepare::uncovered(head, range, |frames| {
                (frames.start / PAGE_FRAMES..=(frames.end - 1) / PAGE_FRAMES).all(|index| {
                    cache.status(PageKey {
                        asset: asset.asset_id(),
                        index,
                    }) == PageStatus::Ready
                })
            })
        });
        if ready {
            Ok(())
        } else {
            asset.mark_cold();
            Err(Error::NotReady)
        }
    }
}
