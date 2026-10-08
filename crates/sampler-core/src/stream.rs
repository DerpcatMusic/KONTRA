//! Bounded decoded-page ownership. One audio cache and one worker coordinator.
use crate::{AssetId, Error, Frame, Index, Pcm};
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
    Timeout,
    DecodeFailed(DecodeFailure),
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
    retries: u8,
    retry_after: Option<std::time::Instant>,
    next: Option<Index>,
    previous_use: Option<Index>,
    next_use: Option<Index>,
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

#[derive(Default)]
struct PageList {
    first: Option<Index>,
    last: Option<Index>,
}

/// Audio-owned page slots. Construction/destruction belong off audio. Protect all
/// current demand before requesting replacements in an epoch, so voice order
/// cannot evict another voice's required page. A cache miss never waits.
pub struct StreamCache {
    entries: Box<[Option<Entry>]>,
    buckets: Box<[Option<Index>]>,
    requests: Producer<Request>,
    completed: Consumer<Completed>,
    recycled: Producer<Box<[Frame]>>,
    epoch: u64,
    protected: usize,
    serial: u64,
    store: u64,
    /// Decoder threads to unpark after a service pushed requests.
    wake: Vec<std::thread::Thread>,
    pushed: bool,
    /// Set by a start that found an asset cold; wakes the reloader.
    cold: std::sync::atomic::AtomicBool,
    vacant: Vec<usize>,
    idle: PageList,
    active: PageList,
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
    /// Nominal streaming polyphony; exact horizon admission also checks shared
    /// demand because pitch and crossfade windows can exceed three pages.
    pub fn voice_budget(&self) -> usize { (self.entries.len() / 3).max(1) }

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
        let buckets = pages.checked_mul(2).and_then(usize::checked_next_power_of_two).ok_or(Error::Capacity)?;
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
                buckets: vec![None; buckets].into_boxed_slice(),
                requests,
                completed,
                recycled,
                epoch: 1,
                protected: 0,
                serial: 0,
                store,
                wake: Vec::new(),
                pushed: false,
                cold: std::sync::atomic::AtomicBool::new(false),
                vacant: (0..pages).rev().collect(),
                idle: PageList::default(),
                active: PageList::default(),
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
    /// Control side: threads serving this cache's worker, unparked (heap
    /// free) after each service that queued requests, so they can park
    /// instead of polling.
    pub fn set_wake(&mut self, threads: Vec<std::thread::Thread>) {
        self.wake = threads;
    }
    /// Unpark the decoder threads if requests were queued since the last call.
    fn wake(&mut self) {
        if std::mem::take(&mut self.pushed)
            | self.cold.swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            for thread in &self.wake {
                thread.unpark();
            }
        }
    }
    pub fn begin_epoch(&mut self) -> Result<(), StreamError> {
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(StreamError::SequenceExhausted)?;
        // All previous protections expire together; splice the whole chain,
        // without visiting its pages. Older idle pages stay ahead of it.
        let active = std::mem::take(&mut self.active);
        if let Some(first) = active.first {
            if let Some(last) = self.idle.last {
                self.entries[last.get()].as_mut().unwrap().next_use = Some(first);
                self.entries[first.get()].as_mut().unwrap().previous_use = Some(last);
            } else {
                self.idle.first = Some(first);
            }
            self.idle.last = active.last;
        }
        self.epoch = epoch;
        self.protected = 0;
        Ok(())
    }
    fn unlink_use(&mut self, slot: usize) {
        let entry = self.entries[slot].as_ref().unwrap();
        let (previous, next) = (entry.previous_use, entry.next_use);
        let list = if entry.used == self.epoch { &mut self.active } else { &mut self.idle };
        if let Some(previous) = previous {
            self.entries[previous.get()].as_mut().unwrap().next_use = next;
        } else { list.first = next; }
        if let Some(next) = next {
            self.entries[next.get()].as_mut().unwrap().previous_use = previous;
        } else { list.last = previous; }
    }
    fn append_active(&mut self, slot: usize) {
        let entry = self.entries[slot].as_mut().unwrap();
        entry.used = self.epoch;
        entry.previous_use = self.active.last;
        entry.next_use = None;
        if let Some(last) = self.active.last {
            self.entries[last.get()].as_mut().unwrap().next_use = Some(Index::new(slot));
        } else { self.active.first = Some(Index::new(slot)); }
        self.active.last = Some(Index::new(slot));
        self.protected += 1;
    }
    fn protect_slot(&mut self, slot: usize) {
        if self.entries[slot].as_ref().unwrap().used != self.epoch {
            self.unlink_use(slot);
            self.append_active(slot);
        }
    }
    fn find(&self, key: PageKey) -> Option<usize> {
        find(&self.entries, &self.buckets, key)
    }
    fn unlink(&mut self, slot: usize) {
        let entry = self.entries[slot].as_ref().unwrap();
        let bucket = bucket(entry.request.key, self.buckets.len());
        let mut at = self.buckets[bucket];
        let mut previous: Option<Index> = None;
        while let Some(index) = at {
            let i = index.get();
            let next = self.entries[i].as_ref().unwrap().next;
            if i == slot {
                if let Some(p) = previous { self.entries[p.get()].as_mut().unwrap().next = next; }
                else { self.buckets[bucket] = next; }
                return;
            }
            previous = Some(index);
            at = next;
        }
        unreachable!("every admitted entry is indexed");
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
        for page in first..=last {
            if let Some(slot) = self.find(PageKey { asset: asset.asset_id(), index: page }) {
                self.protect_slot(slot);
                let entry = self.entries[slot].as_ref().unwrap();
                ready += usize::from(entry.status() == PageStatus::Ready);
            }
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
            let entry = self.entries[slot].as_ref().unwrap();
            if matches!(entry.state, State::Pending) && self.requests.is_abandoned() {
                return Err(StreamError::Disconnected);
            }
            self.protect_slot(slot);
            let entry = self.entries[slot].as_mut().unwrap();
            if matches!(entry.state, State::Pending) && deadline < entry.request.deadline {
                let request = Request {
                    deadline,
                    ..entry.request
                };
                self.requests
                    .push(request)
                    .map_err(|_| StreamError::Capacity)?;
                self.pushed = true;
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
        // Port from v1 0cb7a8a0:src/engine/mod.rs: free.pop()/free.push().
        // Shared decoded pages additionally reuse the oldest idle chain head.
        // Neither allocation nor eviction searches the reserved slot array.
        let slot = self.vacant.last().copied()
            .or_else(|| self.idle.first.map(Index::get))
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
        self.pushed = true;
        self.serial = serial;
        if self.entries[slot].is_some() {
            self.unlink_use(slot);
            self.unlink(slot);
        } else {
            assert_eq!(self.vacant.pop(), Some(slot));
        }
        if let Some(old) = self.entries[slot].take() {
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
            retries: 0,
            retry_after: None,
            next: self.buckets[bucket(key, self.buckets.len())],
            previous_use: None,
            next_use: None,
        });
        let bucket = bucket(key, self.buckets.len());
        self.buckets[bucket] = Some(Index::new(slot));
        self.append_active(slot);
        Ok(PageStatus::Pending)
    }
    /// Client retry policy: transient unavailability gets three retries with
    /// exponential wall-clock backoff. Invalid sample data never retries. Reuse
    /// the protected slot and its generation; no invalidation or audio allocation.
    pub fn request_retry(&mut self, asset: &Pcm, page: usize, deadline: u64) -> Result<PageStatus, StreamError> {
        let key = PageKey { asset: asset.asset_id(), index: page };
        if let Some(slot) = self.find(key) {
            let entry = self.entries[slot].as_mut().unwrap();
            if let State::Failed(error) = entry.state {
                if error != DecodeFailure::Unavailable || entry.retries == 3 {
                    return Err(StreamError::DecodeFailed(error));
                }
                if entry.retry_after.is_some_and(|at| std::time::Instant::now() >= at) {
                    if self.requests.is_abandoned() { return Err(StreamError::Disconnected); }
                    if self.requests.is_full() { return Err(StreamError::Capacity); }
                    self.serial = self.serial.checked_add(1).ok_or(StreamError::SequenceExhausted)?;
                    entry.request.serial = self.serial;
                    entry.request.deadline = deadline;
                    self.requests.push(entry.request).expect("reserved request capacity");
                    entry.retries += 1;
                    entry.state = State::Pending;
                    self.pushed = true;
                }
            }
        }
        self.request(asset, page, deadline)
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
        self.unlink_use(slot);
        self.unlink(slot);
        let entry = self.entries[slot].take().unwrap();
        self.protected -= usize::from(entry.used == self.epoch);
        self.vacant.push(slot);
        if let State::Ready(samples) = entry.state {
            self.recycled
                .push(samples)
                .expect("reserved return capacity");
        }
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
                entry.retry_after = Some(std::time::Instant::now() + std::time::Duration::from_millis(10 << entry.retries));
                Some(PageUpdate::Failed(request.key, error))
            }
        }
    }
    /// The read-only view rendering uses; shareable across render threads.
    pub fn reader(&self) -> PageReader<'_> {
        PageReader {
            entries: &self.entries,
            buckets: &self.buckets,
        }
    }
    pub fn frame(&self, asset: AssetId, frame: usize) -> Option<Frame> {
        self.reader().frame(asset, frame)
    }
    /// Borrow only an entirely resident, contiguous range within one page.
    pub fn span(&self, asset: AssetId, frames: Range<usize>) -> Option<&[Frame]> {
        self.reader().span(asset, frames)
    }
}

// A fixed bucket table and intrusive slot links: admission never shifts all
// resident keys. Collisions only visit their bucket; storage never grows on audio.
fn bucket(key: PageKey, count: usize) -> usize {
    use std::hash::{Hash, Hasher};
    // Keys are process-owned asset revisions and validated page numbers.
    let mut hash = rustc_hash::FxHasher::default();
    key.hash(&mut hash);
    hash.finish() as usize & (count - 1)
}
fn find(entries: &[Option<Entry>], buckets: &[Option<Index>], key: PageKey) -> Option<usize> {
    let mut next = buckets[bucket(key, buckets.len())];
    while let Some(index) = next {
        let i = index.get();
        let entry = entries[i].as_ref().unwrap();
        if entry.request.key == key { return Some(i); }
        next = entry.next;
    }
    None
}

/// Resident pages, read-only.
#[derive(Clone, Copy)]
pub struct PageReader<'a> {
    entries: &'a [Option<Entry>],
    buckets: &'a [Option<Index>],
}
impl<'a> PageReader<'a> {
    fn find(&self, key: PageKey) -> Option<usize> { find(self.entries, self.buckets, key) }
    pub fn frame(&self, asset: AssetId, frame: usize) -> Option<Frame> {
        self.span(asset, frame..frame.checked_add(1)?).map(|s| s[0])
    }
    pub fn span(&self, asset: AssetId, frames: Range<usize>) -> Option<&'a [Frame]> {
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
    /// The horizon admitted with each start, matched to the frontend's service
    /// horizon. Setup side; changing it does not allocate.
    pub fn set_stream_horizon(&mut self, frames: u32) -> Result<(), Error> {
        if frames == 0 { return Err(Error::InvalidInput); }
        self.stream_horizon = frames;
        self.refresh_stream_reservations();
        Ok(())
    }
    pub fn stream_underruns(&self) -> u64 {
        self.stream_underruns
    }

    /// Poll a bounded batch, protect every live source's horizon, then request its
    /// pages with first-use deadlines. No voice/clock advancement or worker waiting.
    /// True means the complete snapshot horizon is resident. Transient decode
    /// failures retry with bounded backoff; corruption and exhausted retries
    /// return an error. Accepted requests survive errors. Requery after events.
    /// Start sources whose first frames are not resident (say, purged start
    /// ranges) silent, fading in once their pages arrive, instead of refusing
    /// them `NotReady`. They still mark the asset cold for reload.
    pub fn set_cold_starts(&mut self, on: bool) {
        self.cold_starts = on;
    }

    /// Offline waits occur only at render boundaries, after due callbacks and
    /// delayed starts. Realtime service never sleeps or waits for storage.
    pub fn set_offline(&mut self, offline: bool) { self.offline = offline; }

    pub fn take_stream_fault(&mut self) -> Option<StreamError> { self.stream_fault.take() }

    pub fn wait_streaming(&mut self, frames: u32, timeout: std::time::Duration) -> Result<(), StreamError> {
        if self.stream_cache.is_none() { return Ok(()); }
        let until = std::time::Instant::now() + timeout;
        loop {
            match self.service_streaming(frames) {
                Ok(true) => return Ok(()),
                Ok(false) => {},
                Err(error) => return Err(error),
            }
            if std::time::Instant::now() >= until { return Err(StreamError::Timeout); }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    }

    /// Constant-size geometry and a preallocated-storage counter, like v1's
    /// free stream slots. Page jobs/wakes belong to block service, not each start.
    pub(crate) fn admit_streaming(&self, cursor: crate::source::Cursor, cold: bool) -> Result<usize, StreamError> {
        let cache = self.stream_cache.as_ref().ok_or(StreamError::NotConfigured)?;
        if cold && cache.requests.is_abandoned() { return Err(StreamError::Disconnected); }
        let pages = cursor.reservation_pages(self.stream_horizon).max(1);
        if self.stream_reserved.checked_add(pages).is_none_or(|n| n > cache.entries.len()) {
            return Err(StreamError::Capacity);
        }
        Ok(pages)
    }

    /// Called in the existing render completion loop, and after pitch edits.
    /// Starts only read the total; they never walk the other voices.
    pub(super) fn refresh_stream_reservation(&mut self, index: usize, step: f64) {
        let voice = self.voices.slots[index].value.as_mut().unwrap();
        if voice.stream_pages == 0 { return; }
        let mut pages = voice.cursor.reservation_pages(self.stream_horizon).max(1);
        if step != voice.cursor.step() {
            pages = pages.max(voice.cursor.with_step(step).reservation_pages(self.stream_horizon));
        }
        self.stream_reserved = self.stream_reserved - voice.stream_pages + pages;
        voice.stream_pages = pages;
    }

    pub(super) fn refresh_stream_reservations(&mut self) {
        if self.stream_cache.is_none() { return; }
        let mut next = self.voices.first;
        while let Some(index) = next {
            next = self.voices.slots[index].next;
            let voice = self.voices.slots[index].value.as_ref().unwrap();
            let family = self.families.get(voice.family.0).unwrap();
            let note = self.notes.get(family.note.0).unwrap();
            let ratio = self.expressions.get(note.expression.0).unwrap().rendered.ratio;
            self.refresh_stream_reservation(index, voice.base_step * ratio);
        }
    }

    pub fn service_streaming(&mut self, frames: u32) -> Result<bool, StreamError> {
        self.now
            .checked_add(u64::from(frames))
            .ok_or(StreamError::ClockOverflow)?;
        let mut cache = self.stream_cache.take().ok_or(StreamError::NotConfigured)?;
        // Temporarily detach only the audio-owned cache to borrow the immutable
        // voice/plan snapshot. The visitor cannot execute callbacks or mutate it.
        let result = self.service_cache(&mut cache, frames);
        if matches!(result, Err(StreamError::DecodeFailed(_) | StreamError::Disconnected)) {
            // A terminal source fault cannot leave a never-started onset held.
            for word in 0..self.voice_activity.len() {
                let mut bits = self.voice_activity[word];
                while bits != 0 {
                    let index = word * 64 + bits.trailing_zeros() as usize;
                    bits &= bits - 1;
                    let Some(voice) = self.voices.slots[index].value else { continue; };
                    if !voice.cursor.holding_onset() { continue; }
                    let note = self.notes.get(self.families.get(voice.family.0).unwrap().note.0).unwrap();
                    let asset = self.plans.get(note.plan.0).unwrap().prepared.pcm[voice.sample].asset_id();
                    let failed = cache.requests.is_abandoned() || cache.entries.iter().flatten().any(|entry| {
                        entry.request.key.asset == asset && matches!(entry.state,
                            State::Failed(error) if error != DecodeFailure::Unavailable || entry.retries == 3)
                    });
                    if failed { self.end_voice(crate::VoiceId(self.voices.id(index))); }
                }
            }
        }
        cache.wake();
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
        let mut first_error = None;
        // Protect all voices before any eviction: admission order must never evict
        // a page that a later voice already needs in this same snapshot horizon.
        for requesting in [false, true] {
            // Only live voices: an idle part scans a few activity words, not every slot.
            let live = self
                .voice_activity
                .iter()
                .enumerate()
                .flat_map(|(word, &bits)| {
                    let mut rest = bits;
                    std::iter::from_fn(move || {
                        (rest != 0).then(|| {
                            let bit = rest.trailing_zeros() as usize;
                            rest &= rest - 1;
                            word * 64 + bit
                        })
                    })
                });
            for index in live {
                let Some(voice) = &self.voices.slots[index].value else {
                    continue;
                };
                let family = self.families.get(voice.family.0).unwrap();
                let note = self.notes.get(family.note.0).unwrap();
                let asset = &self.plans.get(note.plan.0).unwrap().prepared.pcm[voice.sample];
                if asset.resident_frames().is_some() {
                    continue;
                }
                // Publication preserves the generation this demand pass borrows.
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
                            match cache.request_retry(asset, page, deadline) {
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
                if let Some((reach, direction, lead)) = cursor.linear_reach(demand.frames.saturating_sub(1)) {
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
                } else if let Some(reach) = cursor.loop_reach(demand.frames.saturating_sub(1)) {
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
                    first_error.get_or_insert(error);
                    ready = false;
                }
            }
        }
        first_error.map_or(Ok(ready), Err)
    }

    /// Whether a source can start: `Ok(true)` for a cold start (its first
    /// window is not resident, and `set_cold_starts` allows starting it
    /// silent until its pages arrive).
    pub(crate) fn check_source_ready(
        &self,
        asset: &Pcm,
        cursor: crate::source::Cursor,
        envelope: crate::Envelope,
    ) -> Result<bool, Error> {
        if asset.resident_frames().is_some() {
            return Ok(false);
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
            return Ok(false);
        }
        asset.mark_cold();
        cache.cold.store(true, std::sync::atomic::Ordering::Relaxed);
        if self.cold_starts {
            Ok(true)
        } else {
            Err(Error::NotReady)
        }
    }
}

#[cfg(test)]
mod admission_tests {
    use crate::*;

    #[test]
    fn epoch_splices_preserve_protected_pages_and_reuse_returned_slots() {
        let pcm = Pcm::streamed(48000, PAGE_FRAMES * 16).unwrap();
        let (mut cache, mut worker) = StreamCache::new(4).unwrap();
        let fill = |cache: &mut StreamCache, worker: &mut StreamWorker, page| {
            assert_eq!(cache.request(&pcm, page, 0), Ok(PageStatus::Pending));
            let mut job = worker.next_job().unwrap();
            job.frames_mut().fill([page as f32; 2]);
            worker.complete(job, Ok(())).unwrap();
            assert_eq!(cache.poll(), Some(PageUpdate::Loaded(PageKey { asset: pcm.asset_id(), index: page })));
        };
        let check = |cache: &StreamCache| {
            let mut seen = [false; 4];
            let mut protected = 0;
            for (list, active) in [(&cache.idle, false), (&cache.active, true)] {
                let (mut next, mut previous) = (list.first, None);
                while let Some(at) = next {
                    assert!(!std::mem::replace(&mut seen[at.get()], true), "duplicate/cyclic list entry");
                    let entry = cache.entries[at.get()].as_ref().unwrap();
                    assert_eq!(entry.previous_use, previous);
                    assert_eq!(entry.used == cache.epoch, active);
                    protected += usize::from(active);
                    previous = Some(at);
                    next = entry.next_use;
                }
                assert_eq!(previous, list.last);
            }
            assert_eq!(cache.protected, protected);
            for &at in &cache.vacant {
                assert!(!std::mem::replace(&mut seen[at], true), "free slots cannot be linked");
                assert!(cache.entries[at].is_none());
            }
            assert!(seen.into_iter().all(|v| v));
        };
        for page in 0..4 { fill(&mut cache, &mut worker, page); check(&cache); }
        cache.begin_epoch().unwrap();
        check(&cache);
        for page in [0, 2, 3] {
            cache.protect(&pcm, page * PAGE_FRAMES..(page + 1) * PAGE_FRAMES).unwrap();
            check(&cache);
        }
        fill(&mut cache, &mut worker, 4);
        assert_eq!(cache.frame(pcm.asset_id(), PAGE_FRAMES), None);
        check(&cache);
        assert_eq!(cache.request(&pcm, 5, 0), Err(StreamError::Capacity));
        cache.invalidate(PageKey { asset: pcm.asset_id(), index: 2 }).unwrap();
        check(&cache);
        fill(&mut cache, &mut worker, 5);
        cache.begin_epoch().unwrap();
        for page in [0, 4] { cache.protect(&pcm, page * PAGE_FRAMES..(page + 1) * PAGE_FRAMES).unwrap(); }
        fill(&mut cache, &mut worker, 6);
        assert_eq!(cache.frame(pcm.asset_id(), 3 * PAGE_FRAMES), None);
        fill(&mut cache, &mut worker, 7);
        assert_eq!(cache.frame(pcm.asset_id(), 5 * PAGE_FRAMES), None);
        check(&cache);
        // Splice into a nonempty idle list; every page still has exactly one owner.
        cache.begin_epoch().unwrap();
        cache.protect(&pcm, 0..PAGE_FRAMES).unwrap();
        cache.begin_epoch().unwrap();
        check(&cache);
        fill(&mut cache, &mut worker, 8);
        check(&cache);
    }

    #[test]
    fn failed_dsp_starts_do_not_leak_stream_credits_and_end_returns_them() {
        let pcm = Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 32]).unwrap();
        let plan = Prepared::new(48000, vec![pcm], vec![], 0).unwrap();
        let limits = Limits::for_plan(&plan, 8, 1);
        let (cache, _worker) = StreamCache::new(2).unwrap();
        let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
        rt.set_stream_horizon(64).unwrap();
        let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
        let first = rt.note_on(input(1), 60, 1.).unwrap();
        let second = rt.note_on(input(2), 60, 1.).unwrap();
        for _ in 0..1000 {
            let voice = rt.start(first, 0, 0, 1.).unwrap();
            assert_eq!(rt.stream_reserved, 1);
            assert_eq!(rt.start(second, 0, 0, 1.), Err(Error::Capacity));
            assert_eq!(rt.stream_reserved, 1);
            rt.stop_voice(voice).unwrap();
            assert_eq!(rt.stream_reserved, 0);
            let voice = rt.start(second, 0, 0, 1.).unwrap();
            rt.stop_voice(voice).unwrap();
            assert_eq!(rt.stream_reserved, 0);
        }
    }

    #[test]
    fn colliding_page_keys_survive_middle_tail_head_removal_and_slot_reuse() {
        let pcm = Pcm::streamed(48000, PAGE_FRAMES * 512).unwrap();
        let (mut cache, mut worker) = StreamCache::new(4).unwrap();
        let keys: Vec<_> = (0..512).map(|index| PageKey { asset: pcm.asset_id(), index })
            .filter(|&key| super::bucket(key, cache.buckets.len()) == 0).take(6).collect();
        assert_eq!(keys.len(), 6);
        let fill = |cache: &mut StreamCache, worker: &mut StreamWorker, key: PageKey| {
            assert_eq!(cache.request(&pcm, key.index, 0), Ok(PageStatus::Pending));
            let mut job = worker.next_job().unwrap();
            assert_eq!(job.key(), key);
            job.frames_mut().fill([key.index as f32; 2]);
            worker.complete(job, Ok(())).unwrap();
            assert_eq!(cache.poll(), Some(PageUpdate::Loaded(key)));
        };
        for &key in &keys[..4] { fill(&mut cache, &mut worker, key); }
        assert_eq!(cache.protected, 4);
        assert_eq!(cache.request(&pcm, keys[4].index, 0), Err(StreamError::Capacity));
        cache.begin_epoch().unwrap();
        assert_eq!(cache.protect(&pcm, keys[0].index * PAGE_FRAMES..(keys[0].index + 1) * PAGE_FRAMES), Ok(true));
        assert_eq!(cache.invalidate(keys[1]), Ok(true));
        fill(&mut cache, &mut worker, keys[4]);
        for &key in &[keys[0], keys[2], keys[3], keys[4]] {
            assert_eq!(cache.frame(key.asset, key.index * PAGE_FRAMES), Some([key.index as f32; 2]));
        }
        for &key in &[keys[3], keys[0], keys[4]] {
            assert_eq!(cache.invalidate(key), Ok(true));
            assert_eq!(cache.status(key), PageStatus::Missing);
        }
        assert_eq!(cache.protected, 0);
        assert_eq!(cache.request(&pcm, keys[2].index, 0), Ok(PageStatus::Ready));
        assert_eq!(cache.request(&pcm, keys[2].index, 0), Ok(PageStatus::Ready));
        assert_eq!(cache.protected, 1, "duplicate hits consume one protection credit");
        for &key in &[keys[0], keys[1], keys[3]] { fill(&mut cache, &mut worker, key); }
        assert_eq!(cache.protected, 4);
        assert_eq!(cache.request(&pcm, keys[5].index, 0), Err(StreamError::Capacity));
    }
}
