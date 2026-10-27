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
use crate::fmt::SampleFmt;
use crate::sample::{
    ABSENT, PAGE_FRAMES, REQUESTED, RESIDENT, Sample, SampleId, page_count, page_of,
};
use crate::source::Source;

#[derive(Clone, Copy, Debug)]
pub struct StoreConfig {
    /// Bytes of streamed page cache, shared by every loaded sample.
    pub cache_bytes: usize,
    /// Largest frame any loaded sample may have; sizes the cache slots.
    pub max_frame_bytes: usize,
    /// Streamed page requests the audio side may queue between I/O wakeups.
    pub request_capacity: usize,
}

impl StoreConfig {
    /// 64 MiB of cache, frames up to 8 bytes (24-bit stereo uses 6).
    pub const DEFAULT: Self = Self {
        cache_bytes: 64 << 20,
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
        if !pushed {
            self.0.requests_refused.fetch_add(1, Ordering::Relaxed);
        }
        pushed
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
type PinnedPages = HashMap<(BlobKey, usize), Weak<[u8]>>;

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
        let slot_bytes = PAGE_FRAMES as usize * config.max_frame_bytes;
        let slots = (config.cache_bytes / slot_bytes.max(1)).max(1);
        let shared = Arc::new(Shared {
            cache: Cache::new(slots, slot_bytes),
            requests: ArrayQueue::new(config.request_capacity.max(1)),
            stop: AtomicBool::new(false),
            pages_read: AtomicU64::new(0),
            read_errors: AtomicU64::new(0),
            requests_refused: AtomicU64::new(0),
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
        let fmt = key.fmt;
        let frame_bytes = fmt.bytes_per_frame();
        if fmt.channels == 0 || frame_bytes > self.max_frame_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported sample layout {fmt:?}"),
            ));
        }
        let pages = page_count(frames);
        let mut pinned: Vec<Option<Arc<[u8]>>> = vec![None; pages];
        {
            let shared = &mut self.pinned;
            shared.retain(|_, page| page.strong_count() > 0);
            for range in pin {
                let end = range.end.min(frames);
                if range.start >= end {
                    continue;
                }
                for page in page_of(range.start)..=page_of(end - 1) {
                    let Some(cell) = pinned.get_mut(page) else {
                        continue;
                    };
                    if cell.is_some() {
                        continue;
                    }
                    let id = (key.clone(), page);
                    let bytes = match shared.get(&id).and_then(Weak::upgrade) {
                        Some(bytes) => bytes,
                        None => {
                            let first = page as u64 * PAGE_FRAMES;
                            let mut buf =
                                vec![0; (PAGE_FRAMES.min(frames - first)) as usize * frame_bytes];
                            source.read(first, &mut buf)?;
                            let bytes: Arc<[u8]> = buf.into();
                            shared.insert(id, Arc::downgrade(&bytes));
                            bytes
                        }
                    };
                    *cell = Some(bytes);
                }
            }
        }
        let id = SampleId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let sample = Arc::new(Sample {
            id,
            fmt,
            frames,
            source,
            pinned: pinned.into(),
            streamed: (0..pages).map(|_| AtomicU32::new(ABSENT)).collect(),
        });
        // The I/O thread only stops when the store drops, so this cannot fail.
        let _ = self.registered.send((id, Arc::downgrade(&sample)));
        Ok(sample)
    }
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

/// The I/O thread: earliest `due` first, one page per read, clock eviction.
// ponytail: one thread, one page per read, 1 ms poll; coalesce runs and add
// threads or io_uring when C2 shows dropouts at the reference disk speed.
fn serve(shared: &Shared, registrations: &Receiver<(SampleId, Weak<Sample>)>, slot_bytes: usize) {
    let cache = &shared.cache;
    let mut samples: HashMap<SampleId, Weak<Sample>> = HashMap::new();
    let mut queue = BinaryHeap::new();
    let mut owners: Vec<Option<(Weak<Sample>, usize)>> = vec![None; cache.len()];
    let mut hand = 0;
    let mut buf = vec![0u8; slot_bytes];
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
        let Some(Reverse((_, id, page))) = queue.pop() else {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        };
        let page = page as usize;
        let sample = samples.get(&SampleId(id)).and_then(Weak::upgrade);
        let Some(sample) = sample else { continue };
        let Some(entry) = sample.streamed.get(page) else {
            continue;
        };
        if entry.load(Ordering::Relaxed) != REQUESTED {
            continue;
        }
        let first = page as u64 * PAGE_FRAMES;
        let len = (PAGE_FRAMES.min(sample.frames - first)) as usize * sample.fmt.bytes_per_frame();
        let Some(data) = buf.get_mut(..len) else {
            continue;
        };
        if sample.source.read(first, data).is_err() {
            shared.read_errors.fetch_add(1, Ordering::Relaxed);
            entry.store(ABSENT, Ordering::Relaxed);
            continue;
        }
        let victim = clock(cache, &mut hand);
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
        cache.write(victim, tag(id, page as u32), data);
        entry.store(victim + RESIDENT, Ordering::Release);
        if let Some(owner) = owners.get_mut(victim as usize) {
            *owner = Some((Arc::downgrade(&sample), page));
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
