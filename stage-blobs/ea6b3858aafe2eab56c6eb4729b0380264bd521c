//! Direct-from-disk streaming.
//!
//! Each streaming voice owns a [`Slot`]: a single-producer/single-consumer
//! ring of frames in the voice's *virtual* (playback-order) coordinates. The
//! audio thread configures a slot through a seqlock and reads frames the
//! streamer thread has published; the streamer maps virtual frames through
//! the same [`PlayMap`] as the voice, applies loop crossfades and decodes
//! only the blocks it needs. Everything is atomics: no locks, no allocation
//! and no blocking on the audio side. A frame not yet published is an
//! underrun and plays as silence.

use super::map::{LoopMap, PlayMap, Run};
use crate::audio::{Frame, SampleReader, Source};
use std::{
    alloc::Layout,
    collections::{HashMap, VecDeque},
    ptr::NonNull,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Ring capacity in frames: ≈170 ms at 48 kHz and unity pitch, and larger
/// than the widest window one render block can read.
pub(crate) const RING: u64 = 8192;
/// Concurrently streaming voices per bank: one per voice, so every voice can
/// stream. Rings are touched only once used, so idle slots cost no RAM.
pub(crate) const SLOTS: usize = super::MAX_VOICES;
/// Fixed process-wide pool, shared by every part and plugin instance.
/// Each worker owns one stripe of each bank: rings still have one producer.
const THREADS: usize = 4;
/// Slots with fewer frames than this buffered ahead of the voice are served
/// before any other: a starting voice has only its preload to cover the
/// time to its first streamed frames.
const URGENT: u64 = 2 * CHUNK;
/// Frames decoded per slot per streamer pass, so one voice cannot starve others.
const CHUNK: u64 = 2048;
const NO_SAMPLE: u32 = u32::MAX;
/// A streamer thread idle this long hands its rings' pages back to the
/// system; the next voice to stream through one maps it afresh.
const RECLAIM_AFTER: Duration = Duration::from_secs(5);
const POSITION: u64 = (1 << 48) - 1;

/// Seqlock-protected voice configuration plus the ring itself.
pub(crate) struct Slot {
    seq: AtomicU32,
    sample: AtomicU32,
    /// start, end, loop start, loop end, crossfade, flags, wraps, from.
    config: [AtomicU64; 8],
    /// Lowest virtual frame the consumer may still read (published after reading).
    read: AtomicU64,
    /// `tag << 48 | end`: frames `[from, end)` of configuration `tag` are published.
    written: AtomicU64,
    /// `RING` frames in [`Shared::_rings`], which outlives every slot.
    ring: NonNull<AtomicU64>,
    /// The serving thread's wake-up, in [`Shared::wakes`].
    wake: NonNull<Wake>,
}

/// Wakes one streamer thread when one of its slots is configured.
#[derive(Default)]
struct Wake {
    /// Bumped on every configuration, so a thread notices new voices mid-pass.
    epoch: AtomicU32,
    thread: std::sync::OnceLock<std::thread::Thread>,
}

// SAFETY: `ring` and `wake` point into memory owned by the `Shared` that owns
// the slot; the ring is only accessed through atomics and `Wake` is `Sync`.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}

#[derive(Clone, Copy)]
struct Config {
    sample: u32,
    map: PlayMap,
    wraps: u64,
    from: u64,
}

const REVERSE: u64 = 1;
const LOOPED: u64 = 2;
const UNTIL_RELEASE: u64 = 4;
const ALTERNATING: u64 = 8;

impl Slot {
    fn new(ring: NonNull<AtomicU64>, wake: NonNull<Wake>) -> Self {
        Self {
            wake,
            seq: AtomicU32::new(0),
            sample: AtomicU32::new(NO_SAMPLE),
            config: Default::default(),
            read: AtomicU64::new(0),
            written: AtomicU64::new(0),
            ring,
        }
    }

    #[inline]
    fn frame(&self, v: u64) -> &AtomicU64 {
        // SAFETY: in bounds of this slot's `RING` frames, which outlive `self`.
        unsafe { self.ring.add((v % RING) as usize).as_ref() }
    }

    /// Audio thread: (re)start streaming `map` from virtual frame `from`,
    /// for a consumer whose lowest frame still read is `read` (as in
    /// [`Slot::release_below`]: the frame before the voice position, its
    /// cubic's left tap). The streamer fills up to `RING` frames past `read`,
    /// so a `read` too high lets it overwrite that tap.
    /// Returns the tag the consumer must match in [`Slot::published`].
    pub fn configure(&self, sample: u32, map: &PlayMap, wraps: u64, from: u64, read: u64) -> u16 {
        let l = map.looped.unwrap_or(LoopMap {
            start: 0,
            end: 0,
            xfade: 0,
            until_release: false,
            alternating: false,
        });
        let flags = (u64::from(map.reverse) * REVERSE)
            | (u64::from(map.looped.is_some()) * LOOPED)
            | (u64::from(l.until_release) * UNTIL_RELEASE)
            | (u64::from(l.alternating) * ALTERNATING);
        self.write(
            sample,
            [
                map.start, map.end, l.start, l.end, l.xfade, flags, wraps, from,
            ],
            read,
        )
    }

    /// Streamer, while no voice streams through it: give the ring's pages
    /// back, which read as zeros from then on. False if the system refused.
    fn reclaim(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: `sysconf` has no preconditions.
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as usize;
            let start = self.ring.as_ptr() as usize;
            let (lo, hi) = (start.next_multiple_of(page), (start + RING as usize * 8) / page * page);
            // SAFETY: whole pages inside this slot's ring; no voice reads it
            // (the slot is stopped) and only this thread writes it. Zeros are
            // valid `AtomicU64`s.
            hi > lo && unsafe { libc::madvise(lo as *mut _, hi - lo, libc::MADV_DONTNEED) } == 0
        }
        #[cfg(not(target_os = "linux"))]
        false
    }

    /// Audio thread: stop streaming.
    pub fn stop(&self) {
        self.write(NO_SAMPLE, [0; 8], 0);
    }

    fn write(&self, sample: u32, values: [u64; 8], read: u64) -> u16 {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.sample.store(sample, Ordering::Relaxed);
        for (field, value) in self.config.iter().zip(values) {
            field.store(value, Ordering::Relaxed);
        }
        self.read.store(read, Ordering::Relaxed);
        let seq = seq.wrapping_add(2);
        self.seq.store(seq, Ordering::Release);
        if sample != NO_SAMPLE {
            // SAFETY: `wake` outlives the slot (see `Slot::wake`).
            let wake = unsafe { self.wake.as_ref() };
            wake.epoch.fetch_add(1, Ordering::Release);
            // An atomic swap; a futex wake only when the thread sleeps idle.
            if let Some(thread) = wake.thread.get() {
                thread.unpark();
            }
        }
        tag(seq)
    }

    /// Audio thread: end of the published frames for `tag`, if any are.
    #[inline]
    pub fn published(&self, tag: u16) -> Option<u64> {
        let written = self.written.load(Ordering::Acquire);
        ((written >> 48) as u16 == tag).then_some(written & POSITION)
    }

    /// Audio thread: the consumer no longer needs frames below `v`.
    #[inline]
    pub fn release_below(&self, v: u64) {
        self.read.store(v, Ordering::Release);
    }

    /// Audio thread: copy published frames starting at virtual `v`.
    #[inline]
    pub fn copy(&self, v: u64, out: &mut [Frame]) {
        // Published frames are no longer written: the streamer only fills
        // frames past `written`, which sit elsewhere in the ring, and the
        // `Acquire` in `published` orders its stores before this read. So
        // they copy as plain memory, at most two runs around the wrap,
        // rather than one atomic load a frame.
        #[cfg(target_endian = "little")]
        if out.len() as u64 <= RING {
            let start = (v % RING) as usize;
            let first = out.len().min(RING as usize - start);
            let base = self.ring.as_ptr().cast::<Frame>();
            // SAFETY: both runs lie within this slot's `RING` frames; an
            // `AtomicU64` holds a little-endian `[f32; 2]` bit for bit, and
            // no store to these frames races the read (see above).
            unsafe {
                std::ptr::copy_nonoverlapping(base.add(start), out.as_mut_ptr(), first);
                std::ptr::copy_nonoverlapping(base, out.as_mut_ptr().add(first), out.len() - first);
            }
            return;
        }
        for (i, frame) in out.iter_mut().enumerate() {
            let bits = self.frame(v + i as u64).load(Ordering::Relaxed);
            *frame = [
                f32::from_bits(bits as u32),
                f32::from_bits((bits >> 32) as u32),
            ];
        }
    }

    /// Audio thread: `len` published frames from virtual `v` as the ring
    /// holds them, at most two runs around the wrap, borrowed rather than
    /// copied (see [`Slot::copy`] for why plain reads are sound). Borrow
    /// them only until [`Slot::release_below`] passes them.
    #[inline]
    pub fn runs(&self, v: u64, len: usize) -> Option<[&[Frame]; 2]> {
        if !cfg!(target_endian = "little") || len as u64 > RING {
            return None;
        }
        let start = (v % RING) as usize;
        let first = len.min(RING as usize - start);
        let base = self.ring.as_ptr().cast::<Frame>();
        // SAFETY: both runs lie within this slot's `RING` frames; an
        // `AtomicU64` holds a little-endian `[f32; 2]` bit for bit, and no
        // store to published frames races the read.
        unsafe {
            Some([
                std::slice::from_raw_parts(base.add(start), first),
                std::slice::from_raw_parts(base, len - first),
            ])
        }
    }

    /// Streamer: a consistent configuration if it changed since `seen`.
    fn snapshot(&self, seen: u32) -> Option<(u32, Option<Config>)> {
        let seq = self.seq.load(Ordering::Acquire);
        if seq == seen || seq & 1 == 1 {
            return None;
        }
        let sample = self.sample.load(Ordering::Relaxed);
        let [start, end, loop_start, loop_end, xfade, flags, wraps, from] =
            std::array::from_fn(|i| self.config[i].load(Ordering::Relaxed));
        fence(Ordering::Acquire);
        if self.seq.load(Ordering::Relaxed) != seq {
            return None;
        }
        let looped = (flags & LOOPED != 0).then_some(LoopMap {
            start: loop_start,
            end: loop_end,
            xfade,
            until_release: flags & UNTIL_RELEASE != 0,
            alternating: flags & ALTERNATING != 0,
        });
        let map = PlayMap {
            start,
            end,
            reverse: flags & REVERSE != 0,
            looped,
        };
        Some((
            seq,
            (sample != NO_SAMPLE).then_some(Config {
                sample,
                map,
                wraps,
                from,
            }),
        ))
    }
}

fn tag(seq: u32) -> u16 {
    (seq >> 1) as u16
}

/// Streaming thread and the slots it serves; owned by a [`super::Bank`].
pub(crate) struct Streamer {
    shared: Arc<Shared>,
    sources: Arc<[Option<Arc<Source>>]>,
    _pool: Arc<Pool>,
}

struct Shared {
    slots: Box<[Slot]>,
    /// One per thread; slot `i` is served by thread `i % THREADS`.
    wakes: Arc<[Wake]>,
    /// Every slot's ring in one zeroed allocation: the kernel maps pages on
    /// first write, so rings no voice streamed into stay unbacked.
    _rings: Rings,
    stop: AtomicBool,
    rings: Arc<RingUse>,
}

/// Ring memory in use and handed back, for the smart-memory status.
#[derive(Default)]
pub(crate) struct RingUse {
    /// Rings written since they were last reclaimed.
    touched: AtomicU32,
    /// Rings reclaimed and not written since.
    reclaimed: AtomicU32,
}

impl RingUse {
    const BYTES: usize = RING as usize * 8;

    pub fn bytes(&self) -> usize {
        self.touched.load(Ordering::Relaxed) as usize * Self::BYTES
    }

    pub fn freed(&self) -> usize {
        self.reclaimed.load(Ordering::Relaxed) as usize * Self::BYTES
    }
}

/// A slot's ring pages.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Pages {
    /// Never written: the kernel has not mapped them.
    #[default]
    Unused,
    Touched,
    /// Handed back while idle.
    Reclaimed,
}

struct Rings(NonNull<AtomicU64>);

impl Rings {
    const LAYOUT: Layout = match Layout::array::<AtomicU64>(SLOTS * RING as usize) {
        Ok(layout) => layout,
        Err(_) => panic!("ring memory overflows"),
    };

    fn new() -> Self {
        // SAFETY: a nonzero layout; all-zero bits are a valid `AtomicU64`.
        let ptr = unsafe { std::alloc::alloc_zeroed(Self::LAYOUT) };
        match NonNull::new(ptr.cast()) {
            Some(ptr) => Self(ptr),
            None => std::alloc::handle_alloc_error(Self::LAYOUT),
        }
    }
}

impl Drop for Rings {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with the same layout.
        unsafe { std::alloc::dealloc(self.0.as_ptr().cast(), Self::LAYOUT) }
    }
}

// SAFETY: the memory is only accessed through atomics.
unsafe impl Send for Rings {}
unsafe impl Sync for Rings {}

impl Streamer {
    /// `sources[i]` is `Some` for every sample that is not fully resident.
    pub fn spawn(sources: Vec<Option<Arc<Source>>>) -> std::io::Result<Self> {
        let pool = pool()?;
        let rings = Rings::new();
        // SAFETY: slot i's ring is in bounds; Shared retains rings and wakes.
        let ring = |i: usize| unsafe { rings.0.add(i * RING as usize) };
        let wake = |i: usize| NonNull::from(&pool.wakes[i % THREADS]);
        let shared = Arc::new(Shared {
            slots: (0..SLOTS).map(|i| Slot::new(ring(i), wake(i))).collect(),
            wakes: pool.wakes.clone(),
            _rings: rings,
            stop: AtomicBool::new(false),
            rings: Arc::default(),
        });
        let sources: Arc<[Option<Arc<Source>>]> = sources.into();
        for (stripe, sender) in pool.senders.iter().enumerate() {
            let job = Job {
                shared: shared.clone(),
                sources: sources.clone(),
                stripe,
                cursors: (stripe..SLOTS)
                    .step_by(THREADS)
                    .map(|_| Cursor::default())
                    .collect(),
                idle: Instant::now(),
            };
            if sender.send(job).is_err() {
                shared.stop.store(true, Ordering::Release);
                return Err(std::io::Error::other("streaming worker stopped"));
            }
            pool.wakes[stripe].thread.get().unwrap().unpark();
        }
        Ok(Self {
            shared,
            sources,
            _pool: pool,
        })
    }

    pub fn slots(&self) -> &[Slot] {
        &self.shared.slots
    }

    pub fn sources(&self) -> Arc<[Option<Arc<Source>>]> {
        self.sources.clone()
    }

    pub fn rings(&self) -> Arc<RingUse> {
        self.shared.rings.clone()
    }

    /// Bytes of ring memory.
    pub const BYTES: usize = SLOTS * RING as usize * 8;
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        for wake in self.shared.wakes.iter() {
            if let Some(thread) = wake.thread.get() {
                thread.unpark();
            }
        }
    }
}

/// Registered off the audio thread; dropped by its worker after cancellation.
struct Job {
    shared: Arc<Shared>,
    sources: Arc<[Option<Arc<Source>>]>,
    stripe: usize,
    cursors: Vec<Cursor>,
    idle: Instant,
}

struct Pool {
    wakes: Arc<[Wake]>,
    senders: Vec<mpsc::Sender<Job>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Drop for Pool {
    fn drop(&mut self) {
        // Banks retire off the audio thread. Stop the last pool before a DAW
        // can unload this library and unmap the workers' executable code.
        self.senders.clear();
        for wake in self.wakes.iter() {
            if let Some(thread) = wake.thread.get() {
                thread.unpark();
            }
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn pool() -> std::io::Result<Arc<Pool>> {
    static POOL: Mutex<Weak<Pool>> = Mutex::new(Weak::new());
    let mut shared = POOL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pool) = shared.upgrade() {
        return Ok(pool);
    }
    let wakes: Arc<[Wake]> = (0..THREADS).map(|_| Wake::default()).collect();
    let mut pool = Pool {
        wakes: wakes.clone(),
        senders: Vec::new(),
        threads: Vec::new(),
    };
    for stripe in 0..THREADS {
        let (sender, receiver) = mpsc::channel();
        let wake = wakes.clone();
        let thread = std::thread::Builder::new()
            .name(format!("kontakto-stream-{stripe}"))
            .spawn(move || Worker::new().run(receiver, &wake[stripe]))?;
        let _ = wakes[stripe].thread.set(thread.thread().clone());
        pool.senders.push(sender);
        pool.threads.push(thread);
    }
    let pool = Arc::new(pool);
    *shared = Arc::downgrade(&pool);
    Ok(pool)
}

#[derive(Default)]
struct Cursor {
    seq: u32,
    config: Option<Config>,
    next: u64,
    pages: Pages,
}

struct Worker {
    decoded: Decoded,
    frames: Vec<Frame>,
    partners: Vec<Frame>,
}

impl Worker {
    fn new() -> Self {
        let chunk = vec![[0.0; 2]; CHUNK as usize];
        Self {
            decoded: Decoded::default(),
            frames: chunk.clone(),
            partners: chunk,
        }
    }

    fn run(mut self, receiver: mpsc::Receiver<Job>, wake: &Wake) {
        let mut jobs: Vec<Job> = Vec::new();
        loop {
            jobs.extend(receiver.try_iter());
            jobs.retain(|job| !job.shared.stop.load(Ordering::Acquire));
            if jobs.len() > 1 {
                jobs.rotate_left(1);
            }
            let epoch = wake.epoch.load(Ordering::Acquire);
            let (mut busy, mut active, mut touched) = (false, false, false);
            // Urgent work across ALL banks precedes speculative ring fill.
            for lead in [URGENT, RING] {
                for job in &mut jobs {
                    let slots = job.shared.slots.iter().skip(job.stripe).step_by(THREADS);
                    for (slot, cursor) in slots.zip(&mut job.cursors) {
                        if job.shared.stop.load(Ordering::Acquire) {
                            break;
                        }
                        busy |= self.serve(slot, cursor, lead, &job.shared.rings, &job.sources);
                    }
                }
                if wake.epoch.load(Ordering::Acquire) != epoch {
                    break;
                }
            }
            for job in &mut jobs {
                let playing = job.cursors.iter().any(|c| c.config.is_some());
                active |= playing;
                if playing {
                    job.idle = Instant::now();
                }
                if !playing && job.idle.elapsed() >= RECLAIM_AFTER {
                    let slots = job.shared.slots.iter().skip(job.stripe).step_by(THREADS);
                    for (slot, cursor) in slots.zip(&mut job.cursors) {
                        if cursor.pages == Pages::Touched && slot.reclaim() {
                            cursor.pages = Pages::Reclaimed;
                            job.shared.rings.touched.fetch_sub(1, Ordering::Relaxed);
                            job.shared.rings.reclaimed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                touched |= job.cursors.iter().any(|c| c.pages == Pages::Touched);
            }
            if busy || wake.epoch.load(Ordering::Acquire) != epoch {
                continue;
            }
            if active {
                std::thread::park_timeout(Duration::from_millis(1));
            } else {
                // Close readers and release decoded blocks when idle.
                self.decoded.clear();
                if touched {
                    std::thread::park_timeout(RECLAIM_AFTER);
                } else if jobs.is_empty() {
                    match receiver.recv() {
                        Ok(job) => jobs.push(job),
                        Err(_) => return,
                    }
                } else {
                    std::thread::park();
                }
            }
        }
    }

    /// Decode one chunk for a slot with fewer than `lead` frames buffered
    /// ahead of its consumer; true if work was done.
    fn serve(
        &mut self,
        slot: &Slot,
        cursor: &mut Cursor,
        lead: u64,
        rings: &RingUse,
        sources: &[Option<Arc<Source>>],
    ) -> bool {
        if let Some((seq, config)) = slot.snapshot(cursor.seq) {
            *cursor = Cursor {
                seq,
                config,
                next: config.map_or(0, |c| c.from),
                pages: cursor.pages,
            };
        }
        let Some(config) = cursor.config else {
            return false;
        };
        let read = slot.read.load(Ordering::Acquire);
        let end = config.map.len(config.wraps);
        let limit = read.saturating_add(lead.min(RING)).min(end);
        // Refill in whole chunks: a trickle each wakeup costs nearly as much
        // per call as a full chunk. Urgent top-ups take whatever is short.
        if cursor.next >= limit || (lead == RING && limit - cursor.next < CHUNK && limit < end) {
            return false;
        }
        let n = (limit - cursor.next).min(CHUNK);
        let source = sources.get(config.sample as usize).and_then(Option::as_ref);
        self.fill(source, &config, cursor.next, n as usize);
        if cursor.pages != Pages::Touched {
            if cursor.pages == Pages::Reclaimed {
                rings.reclaimed.fetch_sub(1, Ordering::Relaxed);
            }
            cursor.pages = Pages::Touched;
            rings.touched.fetch_add(1, Ordering::Relaxed);
        }
        // A reconfiguration while decoding makes this chunk stale; never publish it.
        if slot.seq.load(Ordering::Acquire) != cursor.seq {
            return true;
        }
        for (i, frame) in self.frames[..n as usize].iter().enumerate() {
            let bits = u64::from(frame[0].to_bits()) | u64::from(frame[1].to_bits()) << 32;
            slot.frame(cursor.next + i as u64)
                .store(bits, Ordering::Relaxed);
        }
        cursor.next += n;
        slot.written.store(
            u64::from(tag(cursor.seq)) << 48 | cursor.next,
            Ordering::Release,
        );
        true
    }

    /// Decode virtual frames `[v, v + n)` into `self.frames`.
    fn fill(&mut self, source: Option<&Arc<Source>>, config: &Config, mut v: u64, n: usize) {
        let mut done = 0;
        while done < n {
            let Some(run) = config.map.run(v, config.wraps) else {
                self.frames[done..n].fill([0.0; 2]);
                return;
            };
            let len = (run.len as usize).min(n - done);
            self.decode_run(source, &run, done, len);
            done += len;
            v += len as u64;
        }
    }

    fn decode_run(&mut self, source: Option<&Arc<Source>>, run: &Run, at: usize, len: usize) {
        let out = &mut self.frames[at..at + len];
        let Some(source) = source else {
            out.fill([0.0; 2]);
            return;
        };
        let low = if run.reverse {
            run.frame + 1 - len as u64
        } else {
            run.frame
        };
        if self.decoded.read(source, low, out).is_err() {
            out.fill([0.0; 2]);
        }
        if run.reverse {
            out.reverse();
        }
        if let Some(blend) = run.blend {
            let partners = &mut self.partners[..len];
            if self.decoded.read(source, blend.partner, partners).is_err() {
                partners.fill([0.0; 2]);
            }
            for (i, (frame, partner)) in out.iter_mut().zip(partners.iter()).enumerate() {
                *frame = blend.apply(i as u64, *frame, *partner);
            }
        }
    }
}

/// Worker-local caches: no lock contention, bounded across every loaded bank.
/// Arc keys keep pointer identities valid until a block/reader is evicted.
const READERS: usize = 64;
const BLOCKS: usize = 128;
#[derive(Default)]
struct Decoded {
    readers: HashMap<usize, (Arc<Source>, SampleReader, u64)>,
    blocks: HashMap<(usize, u64), (Arc<Source>, Box<[Frame]>)>,
    order: VecDeque<(usize, u64)>,
    clock: u64,
    decode_failures: u64,
    // ponytail: READERS recent failures retain repeat counts; raise that ceiling
    // if more concurrent failures need stable suppression. Weak references never
    // keep retired banks alive; oldest reports are evicted when this set fills.
    errors: HashMap<usize, (Weak<Source>, Option<Instant>, u64)>,
    #[cfg(test)]
    decodes: usize,
}

impl Decoded {
    fn clear(&mut self) {
        self.readers.clear();
        self.blocks.clear();
        self.order.clear();
        self.errors.clear();
    }

    fn read(&mut self, source: &Arc<Source>, start: u64, out: &mut [Frame]) -> anyhow::Result<()> {
        let result = self.read_frames(source, start, out);
        if let Err(error) = &result {
            self.decode_failures += 1;
            let id = Arc::as_ptr(source) as usize;
            self.errors.retain(|_, (source, _, _)| source.strong_count() > 0);
            if !self.errors.contains_key(&id) && self.errors.len() == READERS {
                let oldest = *self.errors.iter().min_by_key(|(_, error)| error.1).unwrap().0;
                self.errors.remove(&oldest);
            }
            let (_, last, failures) = self.errors.entry(id).or_insert_with(|| (Arc::downgrade(source), None, 0));
            *failures += 1;
            // First failure of every source is visible; repeats coalesce once/second.
            if last.is_none_or(|last| last.elapsed() >= Duration::from_secs(1)) {
                *last = Some(Instant::now());
                crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "streaming", "decode_failed", serde_json::json!({
                    "path":source.path(), "frame":start, "decode_failures":self.decode_failures, "source_decode_failures":*failures,
                    "reason":format!("{error:#}"), "fallback":"silence",
                }));
            }
        }
        result
    }

    fn read_frames(
        &mut self,
        source: &Arc<Source>,
        mut start: u64,
        mut out: &mut [Frame],
    ) -> anyhow::Result<()> {
        let id = Arc::as_ptr(source) as usize;
        while !out.is_empty() {
            let base = start / CHUNK * CHUNK;
            let key = (id, base);
            if !self.blocks.contains_key(&key) {
                self.clock = self.clock.wrapping_add(1);
                if !self.readers.contains_key(&id) {
                    let reader = source.open_stream()?;
                    if self.readers.len() == READERS {
                        let oldest = *self.readers.iter().min_by_key(|(_, r)| r.2).unwrap().0;
                        self.readers.remove(&oldest);
                    }
                    self.readers
                        .insert(id, (source.clone(), reader, self.clock));
                }
                let (_, reader, used) = self.readers.get_mut(&id).unwrap();
                *used = self.clock;
                let mut frames = vec![[0.0; 2]; CHUNK as usize].into_boxed_slice();
                reader.read(base, &mut frames)?;
                #[cfg(test)]
                {
                    self.decodes += 1;
                }
                if self.blocks.len() == BLOCKS {
                    self.blocks.remove(&self.order.pop_front().unwrap());
                }
                self.blocks.insert(key, (source.clone(), frames));
                self.order.push_back(key);
            }
            let offset = (start - base) as usize;
            let n = out.len().min(CHUNK as usize - offset);
            out[..n].copy_from_slice(&self.blocks[&key].1[offset..offset + n]);
            out = &mut out[n..];
            start += n as u64;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banks_share_workers_and_dropped_jobs_are_released() {
        let a = Streamer::spawn(Vec::new()).unwrap();
        let b = Streamer::spawn(Vec::new()).unwrap();
        assert!(Arc::ptr_eq(&a.shared.wakes, &b.shared.wakes));
        assert_eq!(a.shared.wakes.len(), THREADS);
        let weak = Arc::downgrade(&a.shared);
        drop(a);
        drop(b);
        let until = Instant::now() + Duration::from_secs(2);
        while weak.strong_count() != 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(weak.strong_count(), 0, "workers retained a dropped bank");
    }

    #[test]
    fn decoding_is_shared_and_reader_and_block_caches_are_bounded() {
        let path =
            std::env::temp_dir().join(format!("kontra-stream-cache-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..CHUNK * (BLOCKS as u64 + 2) {
            wav.write_sample(0.25f32).unwrap();
            wav.write_sample(-0.5f32).unwrap();
        }
        wav.finalize().unwrap();
        let mut sources = crate::audio::Sources::default();
        let source = Arc::new(sources.source(&path).unwrap());
        let mut cache = Decoded::default();
        let mut out = [[0.0; 2]; 64];
        cache.read(&source, CHUNK - 20, &mut out).unwrap();
        assert!(out.iter().all(|x| *x == [0.25, -0.5]));
        assert_eq!(cache.decodes, 2);
        cache.read(&source.clone(), CHUNK, &mut out).unwrap();
        assert_eq!(cache.decodes, 2, "another voice decoded a cached block");
        for i in 0..BLOCKS + 2 {
            cache.read(&source, i as u64 * CHUNK, &mut out).unwrap();
        }
        assert_eq!(cache.blocks.len(), BLOCKS);
        for _ in 0..READERS + 2 {
            let source = Arc::new(sources.source(&path).unwrap());
            cache.read(&source, 0, &mut out).unwrap();
        }
        assert_eq!(cache.readers.len(), READERS);
        assert_eq!(cache.blocks.len(), BLOCKS);
        cache
            .read(&source, CHUNK * (BLOCKS as u64 + 2), &mut out)
            .unwrap();
        assert!(out.iter().all(|x| *x == [0.0; 2]));
        cache.clear();
        assert!(cache.blocks.is_empty() && cache.readers.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn copy_reads_across_the_ring_wrap() {
        let ring: Vec<AtomicU64> = (0..RING).map(AtomicU64::new).collect();
        let mut wake = Wake::default();
        let slot = Slot::new(NonNull::from(&ring[0]), NonNull::from(&mut wake));
        let mut out = [[0.0; 2]; 64];
        slot.copy(RING * 3 - 20, &mut out);
        for (i, frame) in out.iter().enumerate() {
            let bits = (RING - 20 + i as u64) % RING;
            assert_eq!(*frame, [f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32)]);
        }
    }
    #[test]
    fn streamed_decode_failure_silences_audio_and_throttles_reports() {
        let _diagnostics = crate::diagnostics::acquire();
        let paths = [0, 1].map(|n| std::env::temp_dir().join(format!("kontra-stream-broken-{}-{n}.wav", std::process::id())));
        let sources = paths.each_ref().map(|path| {
            std::fs::write(path, [0u8; 256]).unwrap();
            Arc::new(crate::audio::Sources::default().source(path).unwrap())
        });
        let mut worker = Worker::new();
        let run = PlayMap { start:0, end:64, reverse:false, looped:None }.run(0, 0).unwrap();
        worker.frames[..64].fill([1.0; 2]);
        worker.decode_run(Some(&sources[0]), &run, 0, 64);
        assert!(worker.frames[..64].iter().all(|f| *f == [0.0; 2]));
        let first_id = Arc::as_ptr(&sources[0]) as usize;
        let first = worker.decoded.errors[&first_id].1.unwrap();
        worker.decode_run(Some(&sources[0]), &run, 0, 64);
        assert_eq!(worker.decoded.errors[&first_id].1, Some(first), "repeated source does not format/log again within one second");
        worker.decode_run(Some(&sources[1]), &run, 0, 64);
        assert_eq!(worker.decoded.decode_failures, 3);
        assert_eq!(worker.decoded.errors.len(), 2);
        assert_eq!(worker.decoded.errors[&first_id].2, 2);
        let snapshot = serde_json::to_value(crate::diagnostics::snapshot()).unwrap();
        let events = snapshot["events"].as_array().unwrap();
        for path in &paths {
            assert_eq!(events.iter().filter(|event| event["code"] == "decode_failed" && event["path"] == path.to_string_lossy().as_ref()).count(), 1,
                "both new sources must be identified immediately, without repeat spam");
        }
        worker.decoded.errors.get_mut(&first_id).unwrap().1 = Some(Instant::now() - Duration::from_secs(2));
        worker.decode_run(Some(&sources[0]), &run, 0, 64);
        let snapshot = serde_json::to_value(crate::diagnostics::snapshot()).unwrap();
        assert!(snapshot["events"].as_array().unwrap().iter().any(|event|
            event["code"] == "decode_failed" && event["path"] == paths[0].to_string_lossy().as_ref()
                && event["data"]["source_decode_failures"] == 3 && event["data"]["decode_failures"] == 4),
            "the next summary includes all suppressed failures");
        let retiring: Vec<_> = (0..READERS + 2).map(|_| Arc::new(crate::audio::Sources::default().source(&paths[1]).unwrap())).collect();
        for source in &retiring { worker.decode_run(Some(source), &run, 0, 64); }
        assert_eq!(worker.decoded.errors.len(), READERS, "bookkeeping never grows past the existing reader-cache ceiling");
        drop(retiring);
        worker.decode_run(Some(&sources[0]), &run, 0, 64);
        assert!(worker.decoded.errors.len() <= sources.len());
        assert!(!worker.decoded.errors.values().any(|(source, _, _)| source.strong_count() == 0), "retired source identities are pruned before reuse");
        worker.decoded.clear();
        assert!(worker.decoded.errors.is_empty());
        drop(sources);
        for path in paths { std::fs::remove_file(path).unwrap(); }
    }

}
