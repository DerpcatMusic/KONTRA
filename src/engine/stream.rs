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
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence},
    },
    thread::JoinHandle,
    time::Duration,
};

/// Ring capacity in frames: ≈170 ms at 48 kHz and unity pitch, and larger
/// than the widest window one render block can read.
pub(crate) const RING: u64 = 8192;
/// Concurrently streaming voices per bank: one per voice, so every voice can
/// stream. Rings are touched only once used, so idle slots cost no RAM.
pub(crate) const SLOTS: usize = super::MAX_VOICES;
/// Streamer threads per bank, each serving an interleaved share of the
/// slots, so one thread waiting on the disk does not stall every voice and
/// the disk sees several requests at once. Idle threads sleep until a voice
/// needs them, so extra threads cost nothing while silent.
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
        });
        let flags = (u64::from(map.reverse) * REVERSE)
            | (u64::from(map.looped.is_some()) * LOOPED)
            | (u64::from(l.until_release) * UNTIL_RELEASE);
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
    threads: Vec<JoinHandle<()>>,
}

struct Shared {
    slots: Box<[Slot]>,
    /// One per thread; slot `i` is served by thread `i % THREADS`.
    wakes: Box<[Wake]>,
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
        let rings = Rings::new();
        // SAFETY: slot `i`'s ring starts in bounds of the allocation.
        let ring = |i: usize| unsafe { rings.0.add(i * RING as usize) };
        let wakes: Box<[Wake]> = (0..THREADS).map(|_| Wake::default()).collect();
        let wake = |i: usize| NonNull::from(&wakes[i % THREADS]);
        let shared = Arc::new(Shared {
            slots: (0..SLOTS).map(|i| Slot::new(ring(i), wake(i))).collect(),
            wakes,
            _rings: rings,
            stop: AtomicBool::new(false),
            rings: Arc::default(),
        });
        let sources: Arc<[Option<Arc<Source>>]> = sources.into();
        let threads = (0..THREADS)
            .map(|stripe| {
                let (shared, sources) = (shared.clone(), sources.clone());
                std::thread::Builder::new()
                    .name("kontakto-stream".into())
                    .spawn(move || Worker::new(sources).run(&shared, stripe))
            })
            .collect::<std::io::Result<Vec<JoinHandle<()>>>>()?;
        for (wake, thread) in shared.wakes.iter().zip(&threads) {
            let _ = wake.thread.set(thread.thread().clone());
        }
        Ok(Self {
            shared,
            sources,
            threads,
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
        for thread in self.threads.drain(..) {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
struct Cursor {
    seq: u32,
    config: Option<Config>,
    next: u64,
    /// The slot's open sample, kept across restarts of the same sample and
    /// closed when the slot stops: one open file per streaming voice.
    reader: Option<(u32, SampleReader)>,
    pages: Pages,
}

struct Worker {
    sources: Arc<[Option<Arc<Source>>]>,
    frames: Vec<Frame>,
    partners: Vec<Frame>,
}

impl Worker {
    fn new(sources: Arc<[Option<Arc<Source>>]>) -> Self {
        let chunk = vec![[0.0; 2]; CHUNK as usize];
        Self {
            sources,
            frames: chunk.clone(),
            partners: chunk,
        }
    }

    /// Serve every `THREADS`th slot from `stripe`.
    /// Each round first tops up slots about to run dry, then fills the rest
    /// one chunk each, restarting as soon as a voice starts. With no voice
    /// streaming the thread sleeps until one does.
    fn run(mut self, shared: &Shared, stripe: usize) {
        let slots = || shared.slots.iter().skip(stripe).step_by(THREADS);
        let wake = &shared.wakes[stripe];
        let mut cursors: Vec<Cursor> = slots().map(|_| Cursor::default()).collect();
        while !shared.stop.load(Ordering::Acquire) {
            let epoch = wake.epoch.load(Ordering::Acquire);
            let (mut busy, mut active) = (false, false);
            for (slot, cursor) in slots().zip(&mut cursors) {
                busy |= self.serve(slot, cursor, URGENT, &shared.rings);
                active |= cursor.config.is_some();
            }
            for (slot, cursor) in slots().zip(&mut cursors) {
                if wake.epoch.load(Ordering::Acquire) != epoch {
                    break;
                }
                busy |= self.serve(slot, cursor, RING, &shared.rings);
            }
            if busy || wake.epoch.load(Ordering::Acquire) != epoch {
                continue;
            }
            if active {
                std::thread::park_timeout(Duration::from_millis(1));
            } else if cfg!(target_os = "linux") && cursors.iter().any(|c| c.pages == Pages::Touched) {
                // A wakeup left from busier times returns at once: count
                // only a whole quiet wait.
                let idle = std::time::Instant::now();
                std::thread::park_timeout(RECLAIM_AFTER);
                if wake.epoch.load(Ordering::Acquire) != epoch || idle.elapsed() < RECLAIM_AFTER {
                    continue;
                }
                let touched = slots().zip(&mut cursors).filter(|(_, c)| c.pages == Pages::Touched);
                for (slot, cursor) in touched {
                    if slot.reclaim() {
                        cursor.pages = Pages::Reclaimed;
                        shared.rings.touched.fetch_sub(1, Ordering::Relaxed);
                        shared.rings.reclaimed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            } else {
                std::thread::park();
            }
        }
    }

    /// Decode one chunk for a slot with fewer than `lead` frames buffered
    /// ahead of its consumer; true if work was done.
    fn serve(&mut self, slot: &Slot, cursor: &mut Cursor, lead: u64, rings: &RingUse) -> bool {
        if let Some((seq, config)) = slot.snapshot(cursor.seq) {
            let reader = cursor
                .reader
                .take()
                .filter(|(sample, _)| config.is_some_and(|c| c.sample == *sample));
            *cursor = Cursor {
                seq,
                config,
                next: config.map_or(0, |c| c.from),
                reader,
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
        if cursor.reader.is_none() {
            cursor.reader = self
                .sources
                .get(config.sample as usize)
                .and_then(Option::as_ref)
                .and_then(|source| source.open_stream().ok())
                .map(|reader| (config.sample, reader));
        }
        let reader = cursor.reader.as_mut().map(|(_, reader)| reader);
        self.fill(reader, &config, cursor.next, n as usize);
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
    fn fill(
        &mut self,
        mut reader: Option<&mut SampleReader>,
        config: &Config,
        mut v: u64,
        n: usize,
    ) {
        let mut done = 0;
        while done < n {
            let Some(run) = config.map.run(v, config.wraps) else {
                self.frames[done..n].fill([0.0; 2]);
                return;
            };
            let len = (run.len as usize).min(n - done);
            self.decode_run(reader.as_deref_mut(), &run, done, len);
            done += len;
            v += len as u64;
        }
    }

    fn decode_run(&mut self, reader: Option<&mut SampleReader>, run: &Run, at: usize, len: usize) {
        let out = &mut self.frames[at..at + len];
        let Some(reader) = reader else {
            out.fill([0.0; 2]);
            return;
        };
        let low = if run.reverse {
            run.frame + 1 - len as u64
        } else {
            run.frame
        };
        if reader.read(low, out).is_err() {
            out.fill([0.0; 2]);
        }
        if run.reverse {
            out.reverse();
        }
        if let Some(blend) = run.blend {
            let partners = &mut self.partners[..len];
            if reader.read(blend.partner, partners).is_err() {
                partners.fill([0.0; 2]);
            }
            for (i, (frame, partner)) in out.iter_mut().zip(partners.iter()).enumerate() {
                *frame = blend.apply(i as u64, *frame, *partner);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
