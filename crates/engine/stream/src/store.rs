//! The process-wide store: content-keyed pinned pages, one streamed page cache,
//! and the I/O thread that fills it earliest deadline first (05-STREAM.md §2).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::io;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_queue::ArrayQueue;

use crate::cache::{Cache, tag};
use crate::fmt::MAX_CHANNELS;
use crate::fmt::SampleFmt;
use crate::sample::{
    ABSENT, Page, REQUESTED, RESIDENT, Sample, SampleId, page_count, page_of, page_shift,
};
use crate::source::Source;

#[derive(Clone, Copy, Debug)]
pub struct StoreConfig {
    /// Bytes of streamed page cache, shared by every loaded sample.
    pub cache_bytes: usize,
    /// Frame bytes a full page is sized for; sizes the cache slots. Wider
    /// frames (more channels) load in shorter pages.
    pub max_frame_bytes: usize,
    /// Streamed page requests the audio side may queue between I/O wakeups.
    pub request_capacity: usize,
}

impl StoreConfig {
    /// 128 MiB of cache, frames up to 8 bytes (24-bit stereo uses 6). The
    /// cache holds what every voice reads next: about 300 voices × (8192
    /// frames ahead + a loop's crossfade partner + the pages just played) ×
    /// 8 bytes, with room for the clock. At 64 MiB, 64 steps of 4-note chords
    /// on Areia 16 Violins (309 voices) re-read 70% of their pages and missed
    /// 34,566 frames; at 128 MiB, none. The OS commits pages as they're used.
    pub const DEFAULT: Self = Self {
        cache_bytes: 128 << 20,
        max_frame_bytes: 8,
        request_capacity: 4096,
    };
}

/// Content identity of a sample: two loads with equal keys share pinned pages.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlobKey {
    /// Stable identity of the container (path, or path plus member).
    pub file: Arc<str>,
    /// Where the sample starts inside it.
    pub offset: u64,
    pub fmt: SampleFmt,
}

pub(crate) struct Request {
    pub(crate) sample: SampleId,
    pub(crate) page: u32,
    pub(crate) due: u64,
}

/// Counters since the store was created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamStats {
    pub pages_read: u64,
    pub read_errors: u64,
    /// Page requests refused because the request ring was full (asked again later).
    pub requests_refused: u64,
}

struct Shared {
    cache: Cache,
    requests: ArrayQueue<Request>,
    stop: AtomicBool,
    pages_read: AtomicU64,
    read_errors: AtomicU64,
    requests_refused: AtomicU64,
    /// Requests queued or being read.
    outstanding: AtomicU64,
}

/// The audio side's handle: reads the cache and queues requests. Cloning is cheap.
#[derive(Clone)]
pub struct StreamClient(Arc<Shared>);

impl StreamClient {
    pub(crate) fn cache(&self) -> &Cache {
        &self.0.cache
    }

    pub(crate) fn push(&self, request: Request) -> bool {
        let pushed = self.0.requests.push(request).is_ok();
        if pushed {
            self.0.outstanding.fetch_add(1, Ordering::AcqRel);
        } else {
            self.0.requests_refused.fetch_add(1, Ordering::Relaxed);
        }
        pushed
    }

    /// Offline rendering only (never the audio thread of a live host): waits
    /// until every page asked for so far is read, or `timeout` passes.
    /// Returns whether the store settled.
    pub fn settle(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while self.0.outstanding.load(Ordering::Acquire) > 0 {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_micros(50));
        }
        true
    }

    #[must_use]
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            pages_read: self.0.pages_read.load(Ordering::Relaxed),
            read_errors: self.0.read_errors.load(Ordering::Relaxed),
            requests_refused: self.0.requests_refused.load(Ordering::Relaxed),
        }
    }
}

/// Pinned pages by (blob, page), shared while any sample holds them.
/// Pinned pages by sample and page; `true` for a page of stored blocks.
type PinnedPages = HashMap<(BlobKey, usize), (Weak<[u8]>, bool)>;

pub struct Store {
    shared: Arc<Shared>,
    pinned: PinnedPages,
    /// New samples, for the I/O thread's lookup table.
    registered: Sender<(SampleId, Weak<Sample>)>,
    next_id: AtomicU32,
    max_frame_bytes: usize,
    io: Option<JoinHandle<()>>,
}

impl Store {
    /// Allocates the cache and starts the I/O thread.
    pub fn new(config: StoreConfig) -> io::Result<Self> {
        let slot_bytes = crate::sample::PAGE_FRAMES as usize * config.max_frame_bytes;
        let slots = (config.cache_bytes / slot_bytes.max(1)).max(1);
        let shared = Arc::new(Shared {
            cache: Cache::new(slots, slot_bytes),
            requests: ArrayQueue::new(config.request_capacity.max(1)),
            stop: AtomicBool::new(false),
            pages_read: AtomicU64::new(0),
            read_errors: AtomicU64::new(0),
            requests_refused: AtomicU64::new(0),
            outstanding: AtomicU64::new(0),
        });
        let (registered, registrations) = channel();
        let io = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("kontra-stream-io".into())
                .spawn(move || serve(&shared, &registrations, slot_bytes))?
        };
        Ok(Self {
            shared,
            pinned: HashMap::new(),
            registered,
            next_id: AtomicU32::new(0),
            max_frame_bytes: config.max_frame_bytes,
            io: Some(io),
        })
    }

    /// log2 of the frames per page for samples in `fmt`.
    fn shift(&self, fmt: &SampleFmt) -> u32 {
        page_shift(fmt.bytes_per_frame(), self.max_frame_bytes)
    }

    /// Bytes of the pages the loaded samples hold in memory, each page once.
    #[must_use]
    pub fn pinned_bytes(&self) -> u64 {
        self.pinned
            .values()
            .filter_map(|(page, _)| page.upgrade())
            .map(|page| page.len() as u64)
            .sum()
    }

    #[must_use]
    pub fn client(&self) -> StreamClient {
        StreamClient(self.shared.clone())
    }

    /// Loader thread: registers a sample of `frames` frames and reads the pages
    /// covering `pin` (the preload law's windows) into memory now. Pages another
    /// load of the same `key` already holds are shared, not read again.
    pub fn load(
        &mut self,
        key: &BlobKey,
        source: Arc<dyn Source>,
        frames: u64,
        pin: &[Range<u64>],
    ) -> io::Result<Arc<Sample>> {
        let request = LoadRequest {
            key: key.clone(),
            source,
            frames,
            pin: pin.to_vec(),
        };
        self.load_many(vec![request])
            .pop()
            .unwrap_or_else(|| Err(io::ErrorKind::InvalidInput.into()))
    }

    /// [`Store::load`] for many samples, their pages read on every core. Each
    /// contiguous run of missing pages is one read, so a codec decodes it once.
    /// Requests for the same sample (key and length) load once, their pins
    /// merged, and share one [`Sample`].
    pub fn load_many(&mut self, requests: Vec<LoadRequest>) -> Vec<io::Result<Arc<Sample>>> {
        let mut unique: Vec<LoadRequest> = Vec::with_capacity(requests.len());
        let mut first: HashMap<(BlobKey, u64), usize> = HashMap::new();
        let slots: Vec<usize> = requests
            .into_iter()
            .map(|r| match first.get(&(r.key.clone(), r.frames)) {
                Some(&i) => {
                    if let Some(u) = unique.get_mut(i) {
                        u.pin.extend(r.pin);
                    }
                    i
                }
                None => {
                    first.insert((r.key.clone(), r.frames), unique.len());
                    unique.push(r);
                    unique.len() - 1
                }
            })
            .collect();
        let loaded = self.load_unique(unique);
        slots
            .iter()
            .map(|&i| match loaded.get(i) {
                Some(Ok(sample)) => Ok(Arc::clone(sample)),
                Some(Err(e)) => Err(io::Error::new(e.kind(), e.to_string())),
                None => Err(io::ErrorKind::InvalidInput.into()),
            })
            .collect()
    }

    fn load_unique(&mut self, requests: Vec<LoadRequest>) -> Vec<io::Result<Arc<Sample>>> {
        self.pinned.retain(|_, (page, _)| page.strong_count() > 0);
        // Pages already held, and the runs each request must read.
        let mut plans = Vec::with_capacity(requests.len());
        for r in &requests {
            let channels = usize::from(r.key.fmt.channels);
            if channels == 0 || channels > MAX_CHANNELS {
                plans.push(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsupported sample layout {:?}", r.key.fmt),
                )));
                continue;
            }
            let shift = self.shift(&r.key.fmt);
            let mut pages: Vec<Option<Page>> = vec![None; page_count(r.frames, shift)];
            let mut wanted = vec![false; pages.len()];
            for range in &r.pin {
                let end = range.end.min(r.frames);
                if range.start < end {
                    for page in page_of(range.start, shift)..=page_of(end - 1, shift) {
                        if let Some(w) = wanted.get_mut(page) {
                            *w = true;
                        }
                    }
                }
            }
            for (page, cell) in pages.iter_mut().enumerate() {
                if wanted.get(page).copied().unwrap_or(false) {
                    *cell = self
                        .pinned
                        .get(&(r.key.clone(), page))
                        .and_then(|(b, blocks)| {
                            let b = b.upgrade()?;
                            Some(if *blocks {
                                Page::Blocks(b)
                            } else {
                                Page::Pcm(b)
                            })
                        });
                }
            }
            let mut runs = Vec::new();
            let mut page = 0;
            while page < pages.len() {
                let missing = |p: usize| {
                    wanted.get(p).copied().unwrap_or(false)
                        && pages.get(p).is_some_and(Option::is_none)
                };
                if missing(page) {
                    let start = page;
                    while missing(page) {
                        page += 1;
                    }
                    runs.push(start..page);
                } else {
                    page += 1;
                }
            }
            plans.push(Ok((pages, runs)));
        }
        let max_frame_bytes = self.max_frame_bytes;
        // Read every run, spread over the machine's cores.
        let jobs: Vec<(usize, Range<usize>)> = plans
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().ok().map(|(_, runs)| (i, runs)))
            .flat_map(|(i, runs)| runs.iter().map(move |run| (i, run.clone())))
            .collect();
        let next = AtomicU32::new(0);
        let workers = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(jobs.len().max(1));
        let read: Vec<Vec<RunRead>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    scope.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let n = next.fetch_add(1, Ordering::Relaxed) as usize;
                            let Some((i, run)) = jobs.get(n) else {
                                break done;
                            };
                            let Some(r) = requests.get(*i) else {
                                continue;
                            };
                            let size =
                                1u64 << page_shift(r.key.fmt.bytes_per_frame(), max_frame_bytes);
                            let first = run.start as u64 * size;
                            let end = (run.end as u64 * size).min(r.frames);
                            let frame_bytes = r.key.fmt.bytes_per_frame();
                            let result = if let Some(per) = blocks_per_page(&*r.source, size) {
                                read_blocks(&*r.source, r.frames, run, per)
                            } else {
                                let mut buf = vec![0; (end - first) as usize * frame_bytes];
                                // Split into pages here, so this thread's heap reuses
                                // the freed run for the pages it keeps.
                                let page_bytes = (size as usize * frame_bytes).max(1);
                                r.source.read(first, &mut buf).map(|()| {
                                    buf.chunks(page_bytes)
                                        .map(|c| Page::Pcm(c.into()))
                                        .collect()
                                })
                            };
                            done.push((*i, run.clone(), result));
                        }
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (i, run, result) in read.into_iter().flatten() {
            let (Some(Ok((pages, _))), Some(r)) = (plans.get_mut(i), requests.get(i)) else {
                continue;
            };
            let read = match result {
                Ok(read) => read,
                Err(e) => {
                    if let Some(plan) = plans.get_mut(i) {
                        *plan = Err(e);
                    }
                    continue;
                }
            };
            for (page, shared) in run.zip(read) {
                let blocks = matches!(shared, Page::Blocks(_));
                self.pinned.insert(
                    (r.key.clone(), page),
                    (Arc::downgrade(shared.bytes()), blocks),
                );
                if let Some(cell) = pages.get_mut(page) {
                    *cell = Some(shared);
                }
            }
        }
        plans
            .into_iter()
            .zip(requests)
            .map(|(plan, r)| {
                let (pages, _) = plan?;
                r.source.loaded();
                let id = SampleId(self.next_id.fetch_add(1, Ordering::Relaxed));
                let sample = Arc::new(Sample {
                    id,
                    fmt: r.key.fmt,
                    frames: r.frames,
                    shift: self.shift(&r.key.fmt),
                    source: r.source,
                    streamed: (0..pages.len()).map(|_| AtomicU32::new(ABSENT)).collect(),
                    pinned: pages.into(),
                });
                // The I/O thread only stops when the store drops, so this cannot fail.
                let _ = self.registered.send((id, Arc::downgrade(&sample)));
                Ok(sample)
            })
            .collect()
    }
}

/// A run of pages read for request `.0`.
type RunRead = (usize, Range<usize>, io::Result<Vec<Page>>);

/// Codec blocks in a page of `size` frames, when `source` keeps its pages
/// as stored blocks: a page must hold whole blocks.
fn blocks_per_page(source: &dyn Source, size: u64) -> Option<usize> {
    source.packing()?;
    let block = ncw::SAMPLES_PER_BLOCK as u64;
    (size >= block && size.is_multiple_of(block)).then_some((size / block) as usize)
}

/// Pages `run` of a `frames`-long `source` as stored blocks, `per` blocks a
/// page; the sample's last page may hold fewer.
fn read_blocks(
    source: &dyn Source,
    frames: u64,
    run: &Range<usize>,
    per: usize,
) -> io::Result<Vec<Page>> {
    let total = frames.div_ceil(ncw::SAMPLES_PER_BLOCK as u64) as usize;
    let (bytes, starts) = source.read_blocks(run.start * per..(run.end * per).min(total))?;
    let blocks = starts.len().saturating_sub(1);
    Ok((0..blocks.div_ceil(per))
        .map(|p| {
            let (lo, hi) = (p * per, ((p + 1) * per).min(blocks));
            let from = starts.get(lo).copied().unwrap_or(0);
            let to = starts.get(hi).copied().unwrap_or(from);
            let rebased: Vec<u32> = starts
                .get(lo..=hi)
                .unwrap_or_default()
                .iter()
                .map(|s| s - from)
                .collect();
            Page::blocks(
                bytes.get(from as usize..to as usize).unwrap_or_default(),
                &rebased,
            )
        })
        .collect())
}

/// One sample for [`Store::load_many`].
pub struct LoadRequest {
    pub key: BlobKey,
    pub source: Arc<dyn Source>,
    pub frames: u64,
    /// Frame ranges to hold in memory (the preload law's windows).
    pub pin: Vec<Range<u64>>,
}

impl Drop for Store {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(io) = self.io.take() {
            io.thread().unpark();
            let _ = io.join();
        }
    }
}

/// Reads in flight at once, each on a reader thread of its own: a disk that
/// seeks can reorder them, and decoding runs on several cores.
const READERS: usize = 4;

/// The most pages one read takes: a requested page and the requested pages
/// right after it in the same sample, so a disk seeks once per run, not once
/// per page.
const RUN: usize = 16;

/// A run of pages a reader fills.
struct Read {
    reader: usize,
    sample: Arc<Sample>,
    id: u32,
    page: usize,
    /// The run's bytes; `len` of them read.
    buf: Vec<u8>,
    len: usize,
    ok: bool,
}

/// The I/O thread: earliest `due` first, a run of requested pages per read,
/// [`READERS`] reads in flight, clock eviction. Only this thread touches the
/// cache's slots; readers only read.
// ponytail: a fixed pool of blocking reads; io_uring if a disk needs more
// in flight than threads give.
fn serve(shared: &Shared, registrations: &Receiver<(SampleId, Weak<Sample>)>, slot_bytes: usize) {
    let (done_tx, done) = channel::<Read>();
    std::thread::scope(|scope| {
        let mut jobs = Vec::with_capacity(READERS);
        let mut idle = Vec::with_capacity(READERS);
        for reader in 0..READERS {
            let (job_tx, job_rx) = channel::<Read>();
            let done_tx = done_tx.clone();
            scope.spawn(move || {
                for mut read in job_rx {
                    let data = read.buf.get_mut(..read.len).unwrap_or_default();
                    read.ok = read
                        .sample
                        .source
                        .read(read.page as u64 * read.sample.page_frames(), data)
                        .is_ok();
                    if done_tx.send(read).is_err() {
                        break;
                    }
                }
            });
            jobs.push(job_tx);
            idle.push((reader, vec![0u8; RUN * slot_bytes]));
        }
        schedule(shared, registrations, &jobs, &done, idle);
        // Dropping the job senders ends the readers.
    });
}

fn schedule(
    shared: &Shared,
    registrations: &Receiver<(SampleId, Weak<Sample>)>,
    jobs: &[Sender<Read>],
    done: &Receiver<Read>,
    mut idle: Vec<(usize, Vec<u8>)>,
) {
    let cache = &shared.cache;
    let mut samples: HashMap<SampleId, Weak<Sample>> = HashMap::new();
    let mut queue = BinaryHeap::new();
    let mut owners: Vec<Option<(Weak<Sample>, usize)>> = vec![None; cache.len()];
    let mut hand = 0;
    // Pages being read: the requests for each, settled when it lands.
    let mut flying: HashMap<(u32, usize), u64> = HashMap::new();
    while !shared.stop.load(Ordering::Relaxed) {
        while let Some(r) = shared.requests.pop() {
            queue.push(Reverse((r.due, r.sample.0, r.page)));
        }
        // After the requests: a sample is registered before any voice can ask for it.
        let mut fresh = registrations.try_iter().peekable();
        if fresh.peek().is_some() {
            samples.retain(|_, s| s.strong_count() > 0);
            samples.extend(fresh);
        }
        while let Ok(read) = done.try_recv() {
            land(shared, &mut owners, &mut hand, &mut flying, &read);
            idle.push((read.reader, read.buf));
        }
        while let Some((reader, buf)) = idle.pop() {
            match next_run(shared, &samples, &mut queue, &mut flying, reader, buf) {
                Ok(read) => {
                    if let Some(job) = jobs.get(read.reader)
                        && let Err(e) = job.send(read)
                    {
                        // A reader that died: its run is lost, never waited on.
                        let read = e.0;
                        land(
                            shared,
                            &mut owners,
                            &mut hand,
                            &mut flying,
                            &Read { ok: false, ..read },
                        );
                    }
                }
                Err(buf) => {
                    idle.push((reader, buf));
                    break;
                }
            }
        }
        if idle.len() < READERS {
            if let Ok(read) = done.recv_timeout(Duration::from_millis(1)) {
                land(shared, &mut owners, &mut hand, &mut flying, &read);
                idle.push((read.reader, read.buf));
            }
        } else {
            std::thread::park_timeout(Duration::from_millis(1));
        }
    }
}

/// The earliest request still to read, with the requested pages after it,
/// as a run for `reader`; `Err` gives `buf` back when none is left.
fn next_run(
    shared: &Shared,
    samples: &HashMap<SampleId, Weak<Sample>>,
    queue: &mut BinaryHeap<Reverse<(u64, u32, u32)>>,
    flying: &mut HashMap<(u32, usize), u64>,
    reader: usize,
    buf: Vec<u8>,
) -> Result<Read, Vec<u8>> {
    while let Some(Reverse((_, id, page))) = queue.pop() {
        let page = page as usize;
        if let Some(waiting) = flying.get_mut(&(id, page)) {
            *waiting += 1;
            continue;
        }
        let sample = samples.get(&SampleId(id)).and_then(Weak::upgrade);
        let requested = |p: usize| {
            sample
                .as_ref()
                .and_then(|s| s.streamed.get(p))
                .is_some_and(|e| e.load(Ordering::Relaxed) == REQUESTED)
        };
        let Some(sample) = sample.as_ref().filter(|_| requested(page)) else {
            shared.outstanding.fetch_sub(1, Ordering::AcqRel);
            continue;
        };
        let pages = 1
            + (1..RUN)
                .take_while(|&k| requested(page + k) && !flying.contains_key(&(id, page + k)))
                .count();
        let size = sample.page_frames();
        let first = page as u64 * size;
        let frames = (size * pages as u64).min(sample.frames - first);
        let len = frames as usize * sample.fmt.bytes_per_frame();
        if len > buf.len() {
            shared.outstanding.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        flying.insert((id, page), 1);
        for p in page + 1..page + pages {
            flying.insert((id, p), 0);
        }
        return Ok(Read {
            reader,
            sample: Arc::clone(sample),
            id,
            page,
            buf,
            len,
            ok: false,
        });
    }
    Err(buf)
}

/// Puts a finished run's pages into the cache, evicting by clock, and
/// settles the requests that waited on them.
fn land(
    shared: &Shared,
    owners: &mut [Option<(Weak<Sample>, usize)>],
    hand: &mut usize,
    flying: &mut HashMap<(u32, usize), u64>,
    read: &Read,
) {
    let cache = &shared.cache;
    let sample = &read.sample;
    let size = sample.page_frames() as usize * sample.fmt.bytes_per_frame();
    let data = read.buf.get(..read.len).unwrap_or_default();
    let pages = data.len().div_ceil(size.max(1));
    for (p, chunk) in (read.page..read.page + pages).zip(data.chunks(size.max(1))) {
        let waiting = flying.remove(&(read.id, p)).unwrap_or(0);
        shared.outstanding.fetch_sub(waiting, Ordering::AcqRel);
        let Some(entry) = sample.streamed.get(p) else {
            continue;
        };
        if !read.ok {
            shared
                .read_errors
                .fetch_add(u64::from(p == read.page), Ordering::Relaxed);
            entry.store(ABSENT, Ordering::Relaxed);
            continue;
        }
        let victim = clock(cache, hand);
        if let Some(Some((old, old_page))) = owners.get_mut(victim as usize).map(Option::take)
            && let Some(old_entry) = old
                .upgrade()
                .as_ref()
                .and_then(|s| s.streamed.get(old_page))
        {
            let _ = old_entry.compare_exchange(
                victim + RESIDENT,
                ABSENT,
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
        cache.write(victim, tag(read.id, p as u32), chunk);
        entry.store(victim + RESIDENT, Ordering::Release);
        if let Some(owner) = owners.get_mut(victim as usize) {
            *owner = Some((Arc::downgrade(sample), p));
        }
        shared.pages_read.fetch_add(1, Ordering::Relaxed);
    }
}

/// Second-chance clock over the cache slots.
fn clock(cache: &Cache, hand: &mut usize) -> u32 {
    let len = cache.len();
    for _ in 0..2 * len {
        let slot = (*hand % len) as u32;
        *hand = hand.wrapping_add(1);
        if !cache.take_touched(slot) {
            return slot;
        }
    }
    let slot = (*hand % len) as u32;
    *hand = hand.wrapping_add(1);
    slot
}
