//! Process-wide, memory-only cache of packed sample-head chunks.
//!
//! A preset swap, or a second instrument that reads the same sample, asks for
//! source pages that were already decoded and packed. Those chunks are reused
//! instead of being read and packed again. Nothing is written to disk: decoded
//! audio stays in process memory (LEGAL.md). Lookups happen on decoder and
//! control workers only; the audio thread never reaches this module.
//!
//! Chunks are cut on the absolute page grid, so every preset that covers a page
//! produces the same `(source, start, frames)` key for it.

use sampler_core::Packed;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

/// Packed payload bytes held across all sources. Excess is evicted least recently used.
const BUDGET_BYTES: usize = 128 << 20;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    source: Arc<str>,
    start: usize,
    frames: usize,
}

struct Entry {
    packed: Arc<Packed>,
    bytes: usize,
    tick: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stats {
    pub hits: u64,
    pub misses: u64,
    pub evicted: u64,
}

/// Owned by value; the process holds one through [`process`].
pub(crate) struct HeadCache {
    budget: usize,
    bytes: usize,
    tick: u64,
    entries: HashMap<Key, Entry>,
    // Recency order, oldest first. Keys are shared with `entries`; eviction pops the front.
    recency: BTreeMap<u64, Key>,
    stats: Stats,
}

impl HeadCache {
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            budget,
            bytes: 0,
            tick: 0,
            entries: HashMap::new(),
            recency: BTreeMap::new(),
            stats: Stats::default(),
        }
    }

    pub(crate) fn get(&mut self, source: &Arc<str>, start: usize, frames: usize) -> Option<Arc<Packed>> {
        let key = Key { source: Arc::clone(source), start, frames };
        self.tick += 1;
        let tick = self.tick;
        match self.entries.get_mut(&key) {
            Some(entry) => {
                self.recency.remove(&entry.tick);
                entry.tick = tick;
                self.recency.insert(tick, key);
                self.stats.hits += 1;
                Some(Arc::clone(&entry.packed))
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    /// Stores one page-bounded chunk. A chunk larger than the whole budget is not kept.
    pub(crate) fn insert(&mut self, source: &Arc<str>, start: usize, packed: Packed) {
        let bytes = packed.bytes();
        if bytes > self.budget {
            return;
        }
        let key = Key { source: Arc::clone(source), start, frames: packed.len() };
        if let Some(old) = self.entries.remove(&key) {
            self.recency.remove(&old.tick);
            self.bytes -= old.bytes;
        }
        while self.bytes + bytes > self.budget {
            let Some((_, victim)) = self.recency.pop_first() else { break };
            if let Some(entry) = self.entries.remove(&victim) {
                self.bytes -= entry.bytes;
                self.stats.evicted += 1;
            }
        }
        self.tick += 1;
        self.recency.insert(self.tick, key.clone());
        self.entries.insert(key, Entry { packed: Arc::new(packed), bytes, tick: self.tick });
        self.bytes += bytes;
    }

    pub(crate) fn stats(&self) -> Stats {
        self.stats
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }
}

/// The process cache shared by every streamed source.
pub(crate) fn process() -> &'static Mutex<HeadCache> {
    static CACHE: OnceLock<Mutex<HeadCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HeadCache::new(BUDGET_BYTES)))
}

/// One source's view of a cache: the cache plus that source's content key.
#[derive(Clone, Copy)]
pub(crate) struct ChunkCache<'a> {
    cache: &'a Mutex<HeadCache>,
    source: &'a Arc<str>,
}

impl<'a> ChunkCache<'a> {
    pub(crate) fn new(cache: &'a Mutex<HeadCache>, source: &'a Arc<str>) -> Self {
        Self { cache, source }
    }
    pub(crate) fn get(&self, start: usize, frames: usize) -> Option<Arc<Packed>> {
        lock(self.cache).get(self.source, start, frames)
    }
    pub(crate) fn put(&self, start: usize, packed: Packed) {
        lock(self.cache).insert(self.source, start, packed);
    }
}

fn lock(cache: &Mutex<HeadCache>) -> std::sync::MutexGuard<'_, HeadCache> {
    cache.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{SampleReader, prepare_head_ranges};
    use sampler_core::{PAGE_FRAMES, Pcm};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const RATE: u32 = 48_000;

    fn value(index: usize) -> sampler_core::Frame {
        // Not representable in 16 bits, so every chunk keeps full f32 width.
        [index as f32 / 3.0, 0.5]
    }

    fn counted_reader(frames: usize, reads: Arc<AtomicUsize>) -> SampleReader {
        SampleReader::custom(RATE, frames, move |start, out| {
            reads.fetch_add(1, Ordering::SeqCst);
            for (i, frame) in out.iter_mut().enumerate() {
                *frame = value(start + i);
            }
            Ok(())
        })
    }

    fn prepare(
        frames: usize,
        ranges: &[std::ops::Range<usize>],
        reads: &Arc<AtomicUsize>,
        cache: ChunkCache<'_>,
    ) -> sampler_core::PreparedRanges {
        let pcm = Pcm::streamed(RATE, frames).unwrap();
        let mut reader = counted_reader(frames, Arc::clone(reads));
        let mut page = vec![[0.; 2]; PAGE_FRAMES];
        prepare_head_ranges(&pcm.head_preparer(), &mut reader, ranges, &mut page, &|| false, None, Some(cache))
            .unwrap()
    }

    /// Swapping A to B reads only the pages B does not share with A.
    #[test]
    fn swap_does_not_redecode_shared_head_chunks() {
        let frames = 4 * PAGE_FRAMES;
        let cache = Mutex::new(HeadCache::new(BUDGET_BYTES));
        let source: Arc<str> = Arc::from("library/pack/kick.nkx|0|1024|identity");
        let shared = ChunkCache::new(&cache, &source);
        let reads = Arc::new(AtomicUsize::new(0));

        // Preset A: pages 0, 1, 2 and a partial page 3.
        let a = prepare(frames, &[0..3 * PAGE_FRAMES + 100], &reads, shared);
        assert_eq!(reads.load(Ordering::SeqCst), 4);
        assert_eq!(cache.lock().unwrap().stats().misses, 4);

        // Preset B: pages 0 and 1 shared, a shorter partial page 2, and a
        // distant range. Only the two new chunks are decoded.
        let before = reads.load(Ordering::SeqCst);
        let b = prepare(frames, &[0..2 * PAGE_FRAMES + 7, 3 * PAGE_FRAMES + 10..3 * PAGE_FRAMES + 60], &reads, shared);
        assert_eq!(reads.load(Ordering::SeqCst) - before, 2);
        let stats = cache.lock().unwrap().stats();
        assert_eq!((stats.hits, stats.misses), (2, 6));

        // Every chunk, cached or freshly decoded, carries the reader's frames.
        for (start, packed) in b.ranges().iter() {
            for i in 0..packed.len() {
                assert_eq!(packed.frame(i), value(start + i), "chunk {start} frame {i}");
            }
        }
        assert!(a.bytes() > 0 && b.bytes() > 0);
    }

    #[test]
    fn least_recently_used_chunk_is_evicted_within_budget() {
        let chunk = |seed: usize| Packed::prepare(&vec![[seed as f32 / 3.0, 0.5]; 64]).finish();
        let (one, two, three) = (chunk(1), chunk(2), chunk(3));
        let budget = one.bytes() + two.bytes();
        let mut cache = HeadCache::new(budget);
        let source: Arc<str> = Arc::from("src");
        cache.insert(&source, 0, one);
        cache.insert(&source, 64, two);
        assert!(cache.get(&source, 0, 64).is_some(), "touch the oldest chunk");
        cache.insert(&source, 128, three);
        assert!(cache.get(&source, 0, 64).is_some(), "recently used survives");
        assert!(cache.get(&source, 64, 64).is_none(), "least recently used is evicted");
        assert_eq!(cache.stats().evicted, 1);
        assert!(cache.bytes() <= budget);
    }
}
