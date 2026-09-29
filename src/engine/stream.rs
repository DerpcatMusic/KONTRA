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
    collections::HashMap,
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
/// Concurrently streaming voices per bank; further voices play their preload only.
pub(crate) const SLOTS: usize = 512;
/// Frames decoded per slot per streamer pass, so one voice cannot starve others.
const CHUNK: u64 = 2048;
/// Open sample readers kept by the streamer.
const OPEN_READERS: usize = 64;
const NO_SAMPLE: u32 = u32::MAX;
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
    ring: Box<[AtomicU64]>,
}

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
    fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            sample: AtomicU32::new(NO_SAMPLE),
            config: Default::default(),
            read: AtomicU64::new(0),
            written: AtomicU64::new(0),
            ring: (0..RING).map(|_| AtomicU64::new(0)).collect(),
        }
    }

    /// Audio thread: (re)start streaming `map` from virtual frame `from`.
    /// Returns the tag the consumer must match in [`Slot::published`].
    pub fn configure(&self, sample: u32, map: &PlayMap, wraps: u64, from: u64, read: u64) -> u16 {
        let l = map.looped.unwrap_or(LoopMap { start: 0, end: 0, xfade: 0, until_release: false });
        let flags = u64::from(map.reverse) * REVERSE
            | u64::from(map.looped.is_some()) * LOOPED
            | u64::from(l.until_release) * UNTIL_RELEASE;
        self.write(sample, [map.start, map.end, l.start, l.end, l.xfade, flags, wraps, from], read)
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
        for (i, frame) in out.iter_mut().enumerate() {
            let bits = self.ring[((v + i as u64) % RING) as usize].load(Ordering::Relaxed);
            *frame = [f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32)];
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
        let map = PlayMap { start, end, reverse: flags & REVERSE != 0, looped };
        Some((seq, (sample != NO_SAMPLE).then_some(Config { sample, map, wraps, from })))
    }
}

fn tag(seq: u32) -> u16 {
    (seq >> 1) as u16
}

/// Streaming thread and the slots it serves; owned by a [`super::Bank`].
pub(crate) struct Streamer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    slots: Box<[Slot]>,
    stop: AtomicBool,
}

impl Streamer {
    /// `sources[i]` is `Some` for every sample that is not fully resident.
    pub fn spawn(sources: Vec<Option<Source>>) -> std::io::Result<Self> {
        let shared = Arc::new(Shared { slots: (0..SLOTS).map(|_| Slot::new()).collect(), stop: AtomicBool::new(false) });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("kontakto-stream".into())
            .spawn(move || Worker::new(sources).run(&worker))?;
        Ok(Self { shared, thread: Some(thread) })
    }

    pub fn slots(&self) -> &[Slot] {
        &self.shared.slots
    }

    /// Bytes of ring memory.
    pub const BYTES: usize = SLOTS * RING as usize * 8;
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
struct Cursor {
    seq: u32,
    config: Option<Config>,
    next: u64,
}

struct Worker {
    sources: Vec<Option<Source>>,
    readers: HashMap<u32, SampleReader>,
    frames: Vec<Frame>,
    partners: Vec<Frame>,
}

impl Worker {
    fn new(sources: Vec<Option<Source>>) -> Self {
        let chunk = vec![[0.0; 2]; CHUNK as usize];
        Self { sources, readers: HashMap::new(), frames: chunk.clone(), partners: chunk }
    }

    fn run(mut self, shared: &Shared) {
        let mut cursors: Vec<Cursor> = shared.slots.iter().map(|_| Cursor::default()).collect();
        while !shared.stop.load(Ordering::Acquire) {
            let mut busy = false;
            for (slot, cursor) in shared.slots.iter().zip(&mut cursors) {
                busy |= self.serve(slot, cursor);
            }
            if !busy {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    /// Decode one chunk ahead of the consumer; true if work was done.
    fn serve(&mut self, slot: &Slot, cursor: &mut Cursor) -> bool {
        if let Some((seq, config)) = slot.snapshot(cursor.seq) {
            *cursor = Cursor { seq, config, next: config.map_or(0, |c| c.from) };
        }
        let Some(config) = cursor.config else { return false };
        let limit = slot.read.load(Ordering::Acquire).saturating_add(RING).min(config.map.len(config.wraps));
        if cursor.next >= limit {
            return false;
        }
        let n = (limit - cursor.next).min(CHUNK);
        self.fill(&config, cursor.next, n as usize);
        // A reconfiguration while decoding makes this chunk stale; never publish it.
        if slot.seq.load(Ordering::Acquire) != cursor.seq {
            return true;
        }
        for (i, frame) in self.frames[..n as usize].iter().enumerate() {
            let bits = u64::from(frame[0].to_bits()) | u64::from(frame[1].to_bits()) << 32;
            slot.ring[((cursor.next + i as u64) % RING) as usize].store(bits, Ordering::Relaxed);
        }
        cursor.next += n;
        slot.written.store(u64::from(tag(cursor.seq)) << 48 | cursor.next, Ordering::Release);
        true
    }

    /// Decode virtual frames `[v, v + n)` into `self.frames`.
    fn fill(&mut self, config: &Config, mut v: u64, n: usize) {
        let mut done = 0;
        while done < n {
            let Some(run) = config.map.run(v, config.wraps) else {
                self.frames[done..n].fill([0.0; 2]);
                return;
            };
            let len = (run.len as usize).min(n - done);
            self.decode_run(config.sample, &run, done, len);
            done += len;
            v += len as u64;
        }
    }

    fn decode_run(&mut self, sample: u32, run: &Run, at: usize, len: usize) {
        let out = &mut self.frames[at..at + len];
        let Some(reader) = reader(&mut self.readers, &self.sources, sample) else {
            out.fill([0.0; 2]);
            return;
        };
        let low = if run.reverse { run.frame + 1 - len as u64 } else { run.frame };
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

fn reader<'a>(
    readers: &'a mut HashMap<u32, SampleReader>,
    sources: &[Option<Source>],
    sample: u32,
) -> Option<&'a mut SampleReader> {
    if !readers.contains_key(&sample) {
        let reader = sources.get(sample as usize)?.as_ref()?.open().ok()?;
        if readers.len() >= OPEN_READERS {
            // ponytail: arbitrary eviction; switch to LRU if reopen cost shows up in profiles.
            let victim = *readers.keys().next()?;
            readers.remove(&victim);
        }
        readers.insert(sample, reader);
    }
    readers.get_mut(&sample)
}
