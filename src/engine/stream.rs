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
/// than the widest contiguous window one render block can read.
pub(crate) const RING: u64 = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum StreamErrorKind { Decode, Unavailable, Canceled, Timeout, UnsupportedWindow, UnrepresentablePitch }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct StreamError {
    pub sample: u32,
    /// First virtual source frame of the failed request (before loop mapping).
    pub frame: u64,
    pub kind: StreamErrorKind,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream {:?} at sample {} frame {}", self.kind, self.sample, self.frame)
    }
}
impl std::error::Error for StreamError {}
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
/// Two maximum output blocks: a short host fragment must still leave a
/// complete following block ready. The budget never depends on pitch.
pub(crate) const SPARSE_TAPS: usize = super::MAX_BLOCK * 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum PacketKind { Taps, High, Ultra }

impl PacketKind {
    fn kernel(quality: i32) -> Result<Self, StreamErrorKind> {
        match quality { 1 => Ok(Self::High), 2 => Ok(Self::Ultra), _ => Err(StreamErrorKind::UnsupportedWindow) }
    }
    fn stored(value: u32) -> Option<Self> {
        match value { 0 => Some(Self::Taps), 1 => Some(Self::High), 2 => Some(Self::Ultra), _ => None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SparseRequest {
    config: u32,
    request: u32,
    bank: usize,
    offset: usize,
    len: usize,
    kind: PacketKind,
}

impl SparseRequest {
    fn key(self) -> u64 { u64::from(self.config) << 32 | u64::from(self.request) }
}

fn kernel_words<'a>(points: &[super::hq::Point], words: &'a mut [u64; SPARSE_TAPS]) -> Result<&'a [u64], StreamErrorKind> {
    if points.is_empty() || points.len() > SPARSE_TAPS / 4 { return Err(StreamErrorKind::UnsupportedWindow); }
    for (point, words) in points.iter().zip(words.as_chunks_mut::<4>().0) {
        point.validate()?;
        words.copy_from_slice(&point.raw());
    }
    Ok(&words[..points.len() * 4])
}

fn packet_error(code: u32) -> Option<StreamErrorKind> {
    match code {
        0 => None, 1 => Some(StreamErrorKind::Decode), 3 => Some(StreamErrorKind::UnsupportedWindow),
        4 => Some(StreamErrorKind::UnrepresentablePitch), 5 => Some(StreamErrorKind::Canceled),
        6 => Some(StreamErrorKind::Timeout), _ => Some(StreamErrorKind::Unavailable),
    }
}

struct SparsePacket {
    ready: AtomicU64,
    len: AtomicU32,
    kind: AtomicU32,
    taps: [AtomicU64; SPARSE_TAPS],
    frames: [AtomicU64; SPARSE_TAPS],
    error: AtomicU32,
}

impl Default for SparsePacket {
    fn default() -> Self {
        Self { ready: AtomicU64::new(0), len: AtomicU32::new(0), kind: AtomicU32::new(0),
            taps: std::array::from_fn(|_| AtomicU64::new(0)),
            frames: std::array::from_fn(|_| AtomicU64::new(0)), error: AtomicU32::new(0) }
    }
}

/// Exact KMP over four-tap frames: each saved atomic is read once, even for
/// repeated floors and long EOF padding. The earliest match retains the most
/// lookahead. Only the worker can change cached taps; its stamp is rechecked
/// by the caller after this scan.
fn sparse_offset(taps: &[u64], saved: &[AtomicU64; SPARSE_TAPS], len: usize) -> Option<usize> {
    if taps.is_empty() || taps.len() > len || !taps.len().is_multiple_of(4) { return None; }
    let frames = taps.len() / 4;
    let frame = |i: usize| &taps[i * 4..i * 4 + 4];
    let mut prefix = [0; SPARSE_TAPS / 4];
    let mut matched = 0;
    for i in 1..frames {
        while matched != 0 && frame(i) != frame(matched) { matched = prefix[matched - 1]; }
        if frame(i) == frame(matched) { matched += 1; }
        prefix[i] = matched;
    }
    matched = 0;
    for i in 0..len / 4 {
        let current: [u64; 4] = std::array::from_fn(|tap| saved[i * 4 + tap].load(Ordering::Relaxed));
        while matched != 0 && current.as_slice() != frame(matched) { matched = prefix[matched - 1]; }
        if current.as_slice() == frame(matched) { matched += 1; }
        if matched == frames { return Some((i + 1 - frames) * 4); }
    }
    None
}

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
    /// Full configuration stamp for dense PCM: the packed u16 tag can wrap.
    dense_published_seq: AtomicU32,
    /// Failed generation and kind; failed decoding is never published as PCM.
    failed: AtomicU64,
    failed_at: AtomicU64,
    /// Audio-owned request seqlock; zero length selects ordinary ring filling.
    sparse_seq: AtomicU32,
    sparse_len: AtomicU32,
    sparse_kind: AtomicU32,
    sparse_taps: [AtomicU64; SPARSE_TAPS],
    sparse_bank: AtomicU32,
    /// Audio-selected cache bank; prefetch fills the opposite bank.
    sparse_used: AtomicU32,
    sparse_packets: [SparsePacket; 2],
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
            dense_published_seq: AtomicU32::new(0),
            failed: AtomicU64::new(0),
            failed_at: AtomicU64::new(0),
            sparse_seq: AtomicU32::new(0),
            sparse_len: AtomicU32::new(0),
            sparse_kind: AtomicU32::new(0),
            sparse_taps: std::array::from_fn(|_| AtomicU64::new(0)),
            sparse_bank: AtomicU32::new(0),
            sparse_used: AtomicU32::new(0),
            sparse_packets: Default::default(),
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

    /// Sole ring producer: reclaim only an explicitly stopped generation.
    /// A failed decoder can still have live, published frames. A restart after
    /// this check clears publication in `write`; this worker finishes reclaiming
    /// before publishing the new generation, so its ring cannot be read yet.
    fn reclaim(&self, stopped_seq: u32) -> bool {
        if self.seq.load(Ordering::Acquire) != stopped_seq
            || self.sample.load(Ordering::Relaxed) != NO_SAMPLE
            || self.seq.load(Ordering::Acquire) != stopped_seq {
            return false;
        }
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
        // A reused u16 ring tag must not expose an ancient publication.
        self.written.store(0, Ordering::Relaxed);
        self.dense_published_seq.store(0, Ordering::Relaxed);
        self.failed.store(0, Ordering::Relaxed);
        self.sparse_seq.store(0, Ordering::Relaxed);
        self.sparse_len.store(0, Ordering::Relaxed);
        self.sparse_kind.store(PacketKind::Taps as u32, Ordering::Relaxed);
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

    /// One bounded readiness check in realtime; offline waits have an explicit
    /// failure outcome instead of rendering missing frames as successful silence.
    pub fn ready(&self, generation: u16, need: u64, blocking: bool) -> Result<bool, StreamErrorKind> {
        self.wait_ready(generation, need, blocking, Duration::from_secs(5))
    }

    pub fn has_sparse(&self) -> bool {
        self.sparse_len.load(Ordering::Relaxed) != 0
    }

    /// Audio thread: publish only needed virtual taps, with `u64::MAX` for
    /// zero padding. Identical waiting plans reuse the same request. Arrays
    /// are prepared with the bank; this path never allocates or decodes.
    pub fn request_sparse(&self, generation: u16, taps: &[u64]) -> Result<SparseRequest, StreamErrorKind> {
        self.request_packet(generation, PacketKind::Taps, taps)
    }

    pub(super) fn request_kernel(&self, generation: u16, quality: i32, points: &[super::hq::Point]) -> Result<SparseRequest, StreamErrorKind> {
        let mut words = [0; SPARSE_TAPS];
        let words = kernel_words(points, &mut words)?;
        self.request_packet(generation, PacketKind::kernel(quality)?, words)
    }

    fn request_packet(&self, generation: u16, kind: PacketKind, taps: &[u64]) -> Result<SparseRequest, StreamErrorKind> {
        let config = self.sparse_config(generation, taps)?;
        if let Some(request) = self.cached_sparse(config, kind, taps) {
            self.sparse_used.store(request.bank as u32, Ordering::Relaxed);
            return Ok(request);
        }
        let request = self.queue_sparse(config, kind, taps)?;
        self.sparse_used.store(request.bank as u32, Ordering::Relaxed);
        Ok(request)
    }

    /// Queue the next trajectory after copying the current packet. Keep the
    /// currently selected bank intact while its opposite is prepared.
    pub fn prefetch_sparse(&self, generation: u16, taps: &[u64]) -> Result<(), StreamErrorKind> {
        self.prefetch_packet(generation, PacketKind::Taps, taps)
    }

    pub(super) fn prefetch_kernel(&self, generation: u16, quality: i32, points: &[super::hq::Point]) -> Result<(), StreamErrorKind> {
        let mut words = [0; SPARSE_TAPS];
        let words = kernel_words(points, &mut words)?;
        self.prefetch_packet(generation, PacketKind::kernel(quality)?, words)
    }

    fn prefetch_packet(&self, generation: u16, kind: PacketKind, taps: &[u64]) -> Result<(), StreamErrorKind> {
        let config = self.sparse_config(generation, taps)?;
        let next = &taps[..taps.len().min(super::MAX_BLOCK * 4)];
        let used = self.sparse_used.load(Ordering::Relaxed) as usize;
        // Preserve useful lookahead during tiny host fragments. Replacing a
        // still-pending packet at every fragment would starve its publication.
        if self.cached_sparse(config, kind, taps).is_some()
            || self.sparse_cache(config, kind, next, Some(used)).is_some()
            || self.pending_sparse(config, kind, next).is_some_and(|request|
                request.bank != used
                    || self.sparse_packets[request.bank].ready.load(Ordering::Acquire) != request.key()) {
            return Ok(());
        }
        self.queue_sparse(config, kind, taps)?;
        Ok(())
    }

    fn sparse_config(&self, generation: u16, taps: &[u64]) -> Result<u32, StreamErrorKind> {
        let config = self.seq.load(Ordering::Acquire);
        if config & 1 != 0 || tag(config) != generation || self.sample.load(Ordering::Relaxed) == NO_SAMPLE {
            return Err(StreamErrorKind::Canceled);
        }
        if taps.is_empty() || taps.len() > SPARSE_TAPS || !taps.len().is_multiple_of(4) { return Err(StreamErrorKind::UnsupportedWindow); }
        Ok(config)
    }

    fn cached_sparse(&self, config: u32, kind: PacketKind, taps: &[u64]) -> Option<SparseRequest> {
        self.sparse_cache(config, kind, taps, None)
    }

    fn sparse_cache(&self, config: u32, kind: PacketKind, taps: &[u64], excluded: Option<usize>) -> Option<SparseRequest> {
        let mut best = None;
        let mut coverage = 0;
        let pending = self.sparse_seq.load(Ordering::Relaxed);
        let pending_bank = self.sparse_bank.load(Ordering::Relaxed) as usize;
        for (bank, packet) in self.sparse_packets.iter().enumerate() {
            if excluded == Some(bank) { continue; }
            let key = packet.ready.load(Ordering::Acquire);
            // A superseded producer may publish after the replacement clears
            // readiness. Its pending bank is stable only for the current stamp.
            if pending != 0 && bank == pending_bank && key as u32 != pending { continue; }
            if key == 0 || key >> 32 != u64::from(config)
                || packet.kind.load(Ordering::Relaxed) != kind as u32 { continue; }
            let len = packet.len.load(Ordering::Relaxed) as usize;
            if let Some(offset) = sparse_offset(taps, &packet.taps, len)
                && packet.ready.load(Ordering::Acquire) == key
                && len - offset > coverage {
                coverage = len - offset;
                best = Some(SparseRequest { config, request:key as u32, bank, offset, len:taps.len(), kind });
            }
        }
        best
    }

    fn queue_sparse(&self, config: u32, kind: PacketKind, taps: &[u64]) -> Result<SparseRequest, StreamErrorKind> {
        if let Some(request) = self.pending_sparse(config, kind, taps) { return Ok(request); }
        let old = self.sparse_seq.load(Ordering::Relaxed);
        // Never alias a request within one configuration. A new configuration
        // starts a fresh epoch under its own full sequence stamp.
        let request = old.checked_add(2).ok_or(StreamErrorKind::Unavailable)?;
        let bank = 1 - self.sparse_used.load(Ordering::Relaxed) as usize;
        self.sparse_seq.store(old + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.sparse_packets[bank].ready.store(0, Ordering::Release);
        for (saved, tap) in self.sparse_taps.iter().zip(taps) { saved.store(*tap, Ordering::Relaxed); }
        self.sparse_len.store(taps.len() as u32, Ordering::Relaxed);
        self.sparse_kind.store(kind as u32, Ordering::Relaxed);
        self.sparse_bank.store(bank as u32, Ordering::Relaxed);
        self.sparse_seq.store(request, Ordering::Release);
        self.wake();
        Ok(SparseRequest { config, request, bank, offset:0, len:taps.len(), kind })
    }

    fn pending_sparse(&self, config: u32, kind: PacketKind, taps: &[u64]) -> Option<SparseRequest> {
        if self.sparse_kind.load(Ordering::Relaxed) != kind as u32 { return None; }
        let old = self.sparse_seq.load(Ordering::Relaxed);
        let len = self.sparse_len.load(Ordering::Relaxed) as usize;
        sparse_offset(taps, &self.sparse_taps, len).map(|offset| SparseRequest { config, request:old,
            bank:self.sparse_bank.load(Ordering::Relaxed) as usize, offset, len:taps.len(), kind })
    }

    pub fn ready_sparse(&self, generation: u16, request: SparseRequest, blocking: bool) -> Result<bool, StreamErrorKind> {
        self.wait_sparse(generation, request, blocking, Duration::from_secs(5))
    }

    fn wait_sparse(&self, generation: u16, request: SparseRequest, blocking: bool, wait: Duration) -> Result<bool, StreamErrorKind> {
        let deadline = blocking.then(|| Instant::now() + wait);
        loop {
            let config = self.seq.load(Ordering::Acquire);
            if config != request.config || config & 1 != 0 || tag(config) != generation
                || self.sample.load(Ordering::Relaxed) == NO_SAMPLE || !self.has_sparse() {
                return Err(StreamErrorKind::Canceled);
            }
            let packet = &self.sparse_packets[request.bank];
            if packet.ready.load(Ordering::Acquire) == request.key() {
                if packet.kind.load(Ordering::Relaxed) != request.kind as u32 { return Err(StreamErrorKind::Canceled); }
                return packet_error(packet.error.load(Ordering::Relaxed)).map_or(Ok(true), Err);
            }
            if self.sparse_seq.load(Ordering::Acquire) != request.request
                || self.sparse_bank.load(Ordering::Relaxed) as usize != request.bank {
                return Err(StreamErrorKind::Canceled);
            }
            let failed = self.failed.load(Ordering::Acquire);
            if failed >> 32 == u64::from(config) && failed as u32 != 0 {
                return Err(if failed as u32 == 1 { StreamErrorKind::Decode } else { StreamErrorKind::Unavailable });
            }
            if !blocking { return Ok(false); }
            if Instant::now() >= deadline.unwrap() { return Err(StreamErrorKind::Timeout); }
            std::thread::yield_now();
        }
    }

    /// Atomic results remain immutable while their bank is retained. Both
    /// stamps are rechecked after copying; cancellation never exposes a
    /// partially overwritten packet as successful PCM.
    pub fn copy_sparse(&self, generation: u16, request: SparseRequest, out: &mut [Frame]) -> bool {
        let packet = &self.sparse_packets[request.bank];
        if self.seq.load(Ordering::Acquire) != request.config || tag(request.config) != generation
            || request.kind != PacketKind::Taps
            || out.len() > request.len
            || packet.ready.load(Ordering::Acquire) != request.key()
            || packet.kind.load(Ordering::Relaxed) != request.kind as u32
            || packet.error.load(Ordering::Relaxed) != 0 {
            out.fill([0.; 2]);
            return false;
        }
        for (frame, saved) in out.iter_mut().zip(&packet.frames[request.offset..]) {
            let bits = saved.load(Ordering::Relaxed);
            *frame = [f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32)];
        }
        let valid = self.seq.load(Ordering::Acquire) == request.config
            && packet.ready.load(Ordering::Acquire) == request.key();
        if !valid { out.fill([0.; 2]); }
        valid
    }

    /// One exact resampled result per metadata quartet; both retained banks
    /// and their full configuration/request stamps remain the sparse owner's.
    pub fn copy_kernel(&self, generation: u16, request: SparseRequest, out: &mut [Frame]) -> bool {
        let packet = &self.sparse_packets[request.bank];
        if self.seq.load(Ordering::Acquire) != request.config || tag(request.config) != generation
            || request.kind == PacketKind::Taps || out.len() > request.len / 4
            || packet.ready.load(Ordering::Acquire) != request.key()
            || packet.kind.load(Ordering::Relaxed) != request.kind as u32
            || packet.error.load(Ordering::Relaxed) != 0 {
            out.fill([0.; 2]);
            return false;
        }
        for (i, frame) in out.iter_mut().enumerate() {
            let bits = packet.frames[request.offset + i * 4].load(Ordering::Relaxed);
            *frame = [f32::from_bits(bits as u32), f32::from_bits((bits >> 32) as u32)];
        }
        let valid = self.seq.load(Ordering::Acquire) == request.config
            && packet.ready.load(Ordering::Acquire) == request.key();
        if !valid { out.fill([0.; 2]); }
        valid
    }

    fn wake(&self) {
        // SAFETY: Shared retains this slot's Wake for its entire lifetime.
        let wake = unsafe { self.wake.as_ref() };
        wake.epoch.fetch_add(1, Ordering::Release);
        if let Some(thread) = wake.thread.get() { thread.unpark(); }
    }

    /// Worker: a coherent sparse request, if active. Reconfiguration is
    /// checked independently by the caller before publishing any result.
    fn sparse_request(&self, taps: &mut [u64; SPARSE_TAPS]) -> Option<(u32, usize, usize, PacketKind)> {
        let seq = self.sparse_seq.load(Ordering::Acquire);
        if seq & 1 != 0 { return None; }
        let len = self.sparse_len.load(Ordering::Relaxed) as usize;
        if len == 0 { return None; }
        for (tap, saved) in taps[..len].iter_mut().zip(&self.sparse_taps) {
            *tap = saved.load(Ordering::Relaxed);
        }
        let bank = self.sparse_bank.load(Ordering::Relaxed) as usize;
        let kind = PacketKind::stored(self.sparse_kind.load(Ordering::Relaxed))?;
        fence(Ordering::Acquire);
        (self.sparse_seq.load(Ordering::Relaxed) == seq).then_some((seq, len, bank, kind))
    }

    fn wait_ready(&self, generation: u16, need: u64, blocking: bool, wait: Duration) -> Result<bool, StreamErrorKind> {
        let deadline = blocking.then(|| Instant::now() + wait);
        loop {
            let seq = self.seq.load(Ordering::Acquire);
            if seq & 1 != 0 || tag(seq) != generation || self.sample.load(Ordering::Relaxed) == NO_SAMPLE {
                return Err(StreamErrorKind::Canceled);
            }
            let failed = self.failed.load(Ordering::Acquire);
            if failed >> 32 == u64::from(seq) && failed as u32 != 0
                && need > self.failed_at.load(Ordering::Relaxed) {
                return Err(if failed as u32 == 1 { StreamErrorKind::Decode } else { StreamErrorKind::Unavailable });
            }
            if self.published(generation).is_some_and(|end| end >= need) { return Ok(true); }
            let Some(deadline) = deadline else { return Ok(false); };
            if Instant::now() >= deadline { return Err(StreamErrorKind::Timeout); }
            std::thread::yield_now();
        }
    }

    /// Audio thread: end of the published frames for `tag`, if any are.
    #[inline]
    pub fn published(&self, generation: u16) -> Option<u64> {
        let seq = self.seq.load(Ordering::Acquire);
        if seq & 1 != 0 || tag(seq) != generation
            || self.dense_published_seq.load(Ordering::Acquire) != seq {
            return None;
        }
        // Each chunk's written Release publishes its ring stores, even when
        // the full configuration stamp already matched the preceding chunk.
        let written = self.written.load(Ordering::Acquire);
        ((written >> 48) as u16 == generation
            && self.dense_published_seq.load(Ordering::Acquire) == seq
            && self.seq.load(Ordering::Acquire) == seq).then_some(written & POSITION)
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
    #[cfg(test)]
    paused: AtomicBool,
    #[cfg(test)]
    paused_workers: AtomicU32,
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
            #[cfg(test)]
            paused: AtomicBool::new(false),
            #[cfg(test)]
            paused_workers: AtomicU32::new(0),
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
    pub const RING_BYTES: usize = SLOTS * RING as usize * 8;
    /// Fixed prepared storage; only ring pages participate in reclamation.
    pub const BYTES: usize = Self::RING_BYTES + SLOTS * std::mem::size_of::<Slot>();
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
    #[cfg(test)]
    before_dense_publish: Option<Arc<AtomicU32>>,
    #[cfg(test)]
    before_sparse_publish: Option<Arc<AtomicU32>>,
    #[cfg(test)]
    after_sparse_validation: Option<Arc<AtomicU32>>,
}

impl Worker {
    fn new() -> Self {
        let chunk = vec![[0.0; 2]; CHUNK as usize];
        Self {
            decoded: Decoded::default(),
            frames: chunk.clone(),
            partners: chunk,
            #[cfg(test)]
            before_dense_publish: None,
            #[cfg(test)]
            before_sparse_publish: None,
            #[cfg(test)]
            after_sparse_validation: None,
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
                    #[cfg(test)]
                    {
                        let bit = 1 << job.stripe;
                        if job.shared.paused.load(Ordering::Acquire) {
                            job.shared.paused_workers.fetch_or(bit, Ordering::Release);
                            continue;
                        }
                        job.shared.paused_workers.fetch_and(!bit, Ordering::Release);
                    }
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
                        if cursor.pages == Pages::Touched && slot.reclaim(cursor.seq) {
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
        // Wide clocks need a bounded set of taps, rather than every skipped
        // frame between them. Keep the dense cursor untouched until the audio
        // thread explicitly reconfigures back to contiguous playback.
        if slot.has_sparse() || slot.sparse_seq.load(Ordering::Acquire) & 1 != 0 {
            let mut taps = [0; SPARSE_TAPS];
            let Some((request, len, bank, kind)) = slot.sparse_request(&mut taps) else { return true; };
            let key = u64::from(cursor.seq) << 32 | u64::from(request);
            let packet = &slot.sparse_packets[bank];
            if packet.ready.load(Ordering::Acquire) == key { return false; }
            let source = sources.get(config.sample as usize).and_then(Option::as_ref);
            let mut decoded = Ok(());
            if kind == PacketKind::Taps {
                for (i, tap) in taps[..len].iter().copied().enumerate() {
                    if tap == u64::MAX {
                        self.frames[i] = [0.; 2];
                    } else if let Err(kind) = self.decoded.read_mapped(source, &config, tap,
                        &mut self.frames[i..i + 1], &mut self.partners[..1]) {
                        decoded = Err(kind);
                        break;
                    }
                }
            } else {
                for (i, words) in taps[..len].as_chunks::<4>().0.iter().enumerate() {
                    let point = super::hq::Point::from_raw([words[0], words[1], words[2], words[3]]);
                    // The per-point terminal belongs to the source clock,
                    // including crops whose counted stream config spans the
                    // complete physical sample.
                    if point.ended || point.floor >= config.map.end {
                        self.frames[i * 4..i * 4 + 4].fill([0.; 2]);
                        continue;
                    }
                    let frame = super::hq::sample(kind as i32, point, |first, out| {
                        if slot.seq.load(Ordering::Acquire) != cursor.seq
                            || slot.sparse_seq.load(Ordering::Acquire) != request { return Err(StreamErrorKind::Canceled); }
                        super::hq::read(point, first, out, |from, frames| {
                            if slot.seq.load(Ordering::Acquire) != cursor.seq
                                || slot.sparse_seq.load(Ordering::Acquire) != request { return Err(StreamErrorKind::Canceled); }
                            let source = source.ok_or(StreamErrorKind::Unavailable)?;
                            self.decoded.read(source, from, frames).map_err(|_| StreamErrorKind::Decode)
                        })
                    });
                    match frame {
                        Ok(frame) => {
                            self.frames[i * 4] = frame;
                            self.frames[i * 4 + 1..i * 4 + 4].fill([0.; 2]);
                        }
                        Err(error) => { decoded = Err(error); break; }
                    }
                }
            }
            #[cfg(test)]
            if let Some(gate) = &self.before_sparse_publish
                && gate.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                let deadline = Instant::now() + Duration::from_secs(5);
                while gate.load(Ordering::Acquire) == 1 {
                    assert!(Instant::now() < deadline, "fixture must release the paused sparse producer");
                    std::thread::yield_now();
                }
            }
            if slot.seq.load(Ordering::Acquire) != cursor.seq
                || slot.sparse_seq.load(Ordering::Acquire) != request { return true; }
            #[cfg(test)]
            if let Some(gate) = &self.after_sparse_validation
                && gate.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                let deadline = Instant::now() + Duration::from_secs(5);
                while gate.load(Ordering::Acquire) == 1 {
                    assert!(Instant::now() < deadline, "fixture must release the validated sparse producer");
                    std::thread::yield_now();
                }
            }
            packet.ready.store(0, Ordering::Release);
            let error = match decoded {
                Ok(()) => {
                    for (saved, frame) in packet.frames.iter().zip(&self.frames[..len]) {
                        saved.store(u64::from(frame[0].to_bits()) | u64::from(frame[1].to_bits()) << 32, Ordering::Relaxed);
                    }
                    0
                }
                Err(StreamErrorKind::Decode) => 1,
                Err(StreamErrorKind::Unavailable) => 2,
                Err(StreamErrorKind::UnsupportedWindow) => 3,
                Err(StreamErrorKind::UnrepresentablePitch) => 4,
                Err(StreamErrorKind::Canceled) => 5,
                Err(StreamErrorKind::Timeout) => 6,
            };
            for (saved, tap) in packet.taps.iter().zip(&taps[..len]) { saved.store(*tap, Ordering::Relaxed); }
            packet.len.store(len as u32, Ordering::Relaxed);
            packet.kind.store(kind as u32, Ordering::Relaxed);
            packet.error.store(error, Ordering::Relaxed);
            packet.ready.store(key, Ordering::Release);
            return true;
        }
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
        let decoded = self.decoded.read_mapped(source, &config, cursor.next,
            &mut self.frames[..n as usize], &mut self.partners[..n as usize]);
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
        if let Err(kind) = decoded {
            let code = if kind == StreamErrorKind::Decode { 1 } else { 2 };
            slot.failed_at.store(cursor.next, Ordering::Relaxed);
            slot.failed.store(u64::from(cursor.seq) << 32 | code, Ordering::Release);
            cursor.config = None;
            return true;
        }
        #[cfg(test)]
        if let Some(gate) = &self.before_dense_publish
            && gate.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while gate.load(Ordering::Acquire) == 1 {
                assert!(Instant::now() < deadline, "fixture must release the paused dense producer");
                std::thread::yield_now();
            }
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
        // A stale writer may finish after configure cleared its publication.
        // It cannot corrupt newer published PCM: this slot has one worker,
        // which observes the new configuration only after this serve returns.
        slot.dense_published_seq.store(cursor.seq, Ordering::Release);
        true
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
    // Decode before evicting, so a failed miss retains every valid block.
    // At most BLOCKS cached buffers plus this single worker-owned scratch.
    spare: Option<Box<[Frame]>>,
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
    /// Decode a bounded virtual window into the worker's borrowed scratch.
    /// Dense fills and sparse points share mapping, padding and error ownership.
    fn read_mapped(&mut self, source: Option<&Arc<Source>>, config: &Config, mut v: u64,
        out: &mut [Frame], partners: &mut [Frame]) -> Result<(), StreamErrorKind> {
        let mut done = 0;
        while done < out.len() {
            let Some(run) = config.map.run(v, config.wraps) else {
                out[done..].fill([0.0; 2]);
                return Ok(());
            };
            let len = (run.len as usize).min(out.len() - done);
            self.read_run(source, &run, &mut out[done..done + len], &mut partners[..len])?;
            done += len;
            v += len as u64;
        }
        Ok(())
    }

    fn read_run(&mut self, source: Option<&Arc<Source>>, run: &Run,
        out: &mut [Frame], partners: &mut [Frame]) -> Result<(), StreamErrorKind> {
        let Some(source) = source else {
            out.fill([0.0; 2]);
            return Err(StreamErrorKind::Unavailable);
        };
        let low = if run.reverse { run.frame + 1 - out.len() as u64 } else { run.frame };
        if self.read(source, low, out).is_err() {
            out.fill([0.0; 2]);
            return Err(StreamErrorKind::Decode);
        }
        if run.reverse { out.reverse(); }
        if let Some(blend) = run.blend {
            let partners = &mut partners[..out.len()];
            if self.read(source, blend.partner, partners).is_err() {
                partners.fill([0.0; 2]);
                return Err(StreamErrorKind::Decode);
            }
            for (i, (frame, partner)) in out.iter_mut().zip(partners.iter()).enumerate() {
                *frame = blend.apply(i as u64, *frame, *partner);
            }
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.readers.clear();
        self.blocks.clear();
        self.order.clear();
        self.spare = None;
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
                // Physical HQ support may extend past the complete sample.
                // Avoid codec seeks for a wholly empty block; partial EOF is
                // already padded by SampleReader::read from this same header.
                if base >= reader.frames {
                    out.fill([0.; 2]);
                    return Ok(());
                }
                let mut frames = self.spare.take()
                    .unwrap_or_else(|| vec![[0.0; 2]; CHUNK as usize].into_boxed_slice());
                if let Err(error) = reader.read(base, &mut frames) {
                    self.spare = Some(frames);
                    return Err(error);
                }
                #[cfg(test)]
                {
                    self.decodes += 1;
                }
                if self.blocks.len() == BLOCKS {
                    self.spare = self.blocks.remove(&self.order.pop_front().unwrap())
                        .map(|(_, frames)| frames);
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
pub(super) mod tests {
    use super::*;

    #[path = "dense_generation_tests.rs"]
    mod dense_generation;

    #[path = "kernel_prefetch_tests.rs"]
    mod kernel_prefetch;

    #[path = "cache_reuse_tests.rs"]
    mod cache_reuse;

    #[path = "publication_tests.rs"]
    mod publication;

    /// Deterministic real-disk readiness for sibling Engine PCM fixtures.
    /// Construct before notes: paused background cursors have untouched pages,
    /// and this fixture worker is the sole producer until the bank is retired.
    pub(crate) struct ManualWorker {
        shared: Arc<Shared>,
        sources: Arc<[Option<Arc<Source>>]>,
        worker: Worker,
        cursors: Vec<Cursor>,
    }

    impl ManualWorker {
        pub(crate) fn paused(bank: &crate::engine::Bank) -> Self {
            Self {
                shared: pause(bank),
                sources: bank.streamer.as_ref().unwrap().sources(),
                worker: Worker::new(),
                cursors: (0..SLOTS).map(|_| Cursor::default()).collect(),
            }
        }

        /// Decode only this actual slot, off the audio allocation guard. The
        /// current ring fits four chunks; a sparse request publishes once.
        pub(crate) fn serve(&mut self, slot: u16) {
            let slot = slot as usize;
            for _ in 0..=RING / CHUNK {
                if !self.worker.serve(&self.shared.slots[slot], &mut self.cursors[slot],
                    RING, &self.shared.rings, &self.sources) {
                    break;
                }
            }
        }
    }

    fn wake(shared: &Shared) {
        for wake in shared.wakes.iter() {
            wake.epoch.fetch_add(1, Ordering::Release);
            wake.thread.get().unwrap().unpark();
        }
    }

    fn pause(bank: &crate::engine::Bank) -> Arc<Shared> {
        let shared = bank.streamer.as_ref().unwrap().shared.clone();
        shared.paused.store(true, Ordering::Release);
        wake(&shared);
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.paused_workers.load(Ordering::Acquire) != (1 << THREADS) - 1 {
            assert!(Instant::now() < deadline, "all workers must acknowledge the fixture gate");
            std::thread::yield_now();
        }
        shared
    }

    fn instrument(path: &std::path::Path, silent: bool) -> crate::import::Instrument {
        let mut wav = hound::WavWriter::create(path, hound::WavSpec { channels:2,
            sample_rate:48_000, bits_per_sample:32, sample_format:hound::SampleFormat::Float }).unwrap();
        for frame in 0..131072 {
            let value = if silent { 0. } else { 0.25 * (frame as f32 * 0.037).sin() };
            wav.write_sample(value).unwrap(); wav.write_sample(-value).unwrap();
        }
        wav.finalize().unwrap();
        crate::import::Instrument { groups:vec![crate::import::Group::default()],
            zones:vec![crate::import::Zone { sample:path.into(), ..Default::default() }], ..Default::default() }
    }

    #[test]
    fn pending_onset_waits_for_actual_pitch_window_without_consuming_any_attack_clock() {
        use crate::{engine::{Bank, Engine}, import::{ModAssignment, ModSource, ModTarget}};
        let dir = tempfile::tempdir().unwrap();
        let mut source = instrument(&dir.path().join("onset.wav"), false);
        source.zones[0].tune = 16.;
        source.groups[0].volume_env = Some(crate::import::Ahdsr { attack_curve:0., attack_ms:7., decay_ms:13.,
            sustain:0.6, release_ms:11., hold_ms:0., unknown_flag:0, unknown_tail:vec![] });
        source.groups[0].mods = [(ModSource::MidiCc(1), ModTarget::Pitch, 1., 13),
            (ModSource::MidiCc(11), ModTarget::Volume, 0.5, 7)].into_iter().map(|(cc,target,intensity,lag_ms)|
                ModAssignment { cc_default: None, name:"Onset".into(), source:cc, target, intensity, lag_ms,
                    invert:false, shaper:None }).collect();
        let frames = crate::audio::decode(&source.zones[0].sample, 131072).unwrap();
        let bank = Bank::load_bare(&source).unwrap();
        let shared = pause(&bank);
        let ram = Bank::from_samples(source.groups.clone(), source.zones.clone(),
            vec![(source.zones[0].sample.clone(), crate::audio::Sample { rate:frames.rate, frames:frames.frames.clone() })]).unwrap();
        let (mut actual, mut oracle) = (Engine::default(), Engine::default());
        actual.set_bank(Some(Box::new(bank))); oracle.set_bank(Some(Box::new(ram)));
        actual.note_on(0,60,127); oracle.note_on(0,60,127);
        let voice = &actual.player.voices[0];
        let stream = voice.stream.unwrap();
        let initial_env = voice.env.level();
        let slot = &shared.slots[stream.slot as usize];
        // Publish genuine source PCM, but only the old fixed1024-frame lead.
        for (i, frame) in frames.frames.iter().take(1024).enumerate() {
            slot.frame(i as u64).store(u64::from(frame[0].to_bits()) | u64::from(frame[1].to_bits()) << 32, Ordering::Relaxed);
        }
        slot.written.store(u64::from(stream.tag) << 48 | 1024, Ordering::Release);
        slot.dense_published_seq.store(slot.seq.load(Ordering::Acquire), Ordering::Release);
        let (mut left, mut right, mut expected_l, mut expected_r) = ([0.;128], [0.;128], [0.;128], [0.;128]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            for _ in 0..64 {
                actual.render(&mut left,&mut right);
                assert_eq!((left,right),([0.;128],[0.;128]));
                let voice = &actual.player.voices[0];
                assert!(voice.onset == super::super::voice::Onset::Waiting && voice.pitch.0.is_nan());
                assert_eq!(voice.pos,0.); assert_eq!(voice.env.level(),initial_env);
            }
            // Later input retargets the authored lag, without aging its source.
            for engine in [&mut actual,&mut oracle] { engine.cc(0,1,127); engine.cc(0,11,64); }
        }),0);
        actual.blocking_streams = true;
        let resume = shared.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            resume.paused.store(false,Ordering::Release); wake(&resume);
        });
        assert_eq!(crate::plugin::tests::allocations(|| {
            for block in 0..24 {
                let cuts: &[usize] = if block % 2 == 0 { &[0,128] } else { &[0,7,36,37,128] };
                for span in cuts.windows(2) {
                    actual.render_checked(&mut left[span[0]..span[1]],&mut right[span[0]..span[1]]).unwrap();
                    oracle.render_checked(&mut expected_l[span[0]..span[1]],&mut expected_r[span[0]..span[1]]).unwrap();
                }
                assert_eq!(left,expected_l,"delayed onset left block{block}");
                assert_eq!(right,expected_r,"delayed onset right block{block}");
            }
        }),0);
        worker.join().unwrap();
        assert_eq!(actual.underruns(),0);
        assert!(left.iter().chain(&right).any(|v| v.abs() > 0.01));
        assert!(actual.player.voices[0].onset == super::super::voice::Onset::Started);
    }

    #[test]
    fn pending_attacks_cancel_on_release_and_reset_without_poisoning_reused_streams() {
        use crate::engine::{Bank, Engine, HostNote, HostPattern};
        let dir = tempfile::tempdir().unwrap();
        let source = instrument(&dir.path().join("cancel.wav"),false);
        let bank = Bank::load_bare(&source).unwrap();
        let shared = pause(&bank);
        let mut engine = Engine { attack: 0., ..Default::default() };
        engine.set_bank(Some(Box::new(bank)));
        let (mut left,mut right) = ([0.;128],[0.;128]);
        let note = HostNote { port:0, channel:0, key:60, id:19, clap:true };
        let pattern = HostPattern { port:0, channel:0, key:60, id:19, clap:true };
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(engine.host_note_on(note,0,60,127,0.));
            engine.render(&mut left,&mut right);
            assert!(engine.player.voices[0].onset == super::super::voice::Onset::Waiting);
        }),0);
        // Data arriving before key-up cannot resurrect an attack that already
        // waited. Publish genuine PCM while the worker remains paused.
        let frames = crate::audio::decode(&source.zones[0].sample,131072).unwrap();
        let stream = engine.player.voices[0].stream.unwrap();
        let slot = &shared.slots[stream.slot as usize];
        for (i, frame) in frames.frames.iter().take(256).enumerate() {
            slot.frame(i as u64).store(u64::from(frame[0].to_bits()) | u64::from(frame[1].to_bits()) << 32,Ordering::Relaxed);
        }
        slot.written.store(u64::from(stream.tag) << 48 | 256,Ordering::Release);
        slot.dense_published_seq.store(slot.seq.load(Ordering::Acquire), Ordering::Release);
        let canceled_at = engine.player.now;
        assert_eq!(crate::plugin::tests::allocations(|| {
            engine.host_note_off(pattern); engine.render(&mut left,&mut right);
            assert_eq!(engine.active_voices(),0,"physical release cancels its unheard attack");
            assert_eq!((left,right),([0.;128],[0.;128]));
            engine.mark_host_notes();
            assert_eq!(engine.host_note_end_at(note),Some(canceled_at),"cold cancellation ends at its actual zero-frame boundary");
            engine.note_on(0,60,127); engine.render(&mut left,&mut right);
            engine.reset(48000.);
            assert_eq!(engine.active_voices(),0,"reset retires pending rows and slots");
            engine.cc(0,64,127); engine.note_on(0,60,127); engine.note_off(0,60);
            engine.render(&mut left,&mut right);
            assert_eq!(engine.active_voices(),1,"the authored pedal retains its pending root");
            assert_eq!((left,right),([0.;128],[0.;128]));
            assert!(engine.take_stream_error().is_none());
        }),0);
        shared.paused.store(false,Ordering::Release); wake(&shared);
        engine.blocking_streams = true;
        assert_eq!(crate::plugin::tests::allocations(|| {
            engine.render_checked(&mut left,&mut right).unwrap();
            assert!(left.iter().chain(&right).any(|v| v.abs() > 0.01));
            engine.cc(0,64,0);
        }),0);
        let mut released = crate::import::Instrument { groups:source.groups.clone(),
            zones:source.zones.clone(), ..Default::default() };
        released.groups[0].release_trigger = true;
        let bank = Bank::load_bare(&released).unwrap();
        let release_shared = pause(&bank);
        let mut release = Engine { attack: 0., ..Default::default() };
        release.set_bank(Some(Box::new(bank)));
        assert_eq!(crate::plugin::tests::allocations(|| {
            release.note_on(0,60,127);
            assert_eq!(release.active_voices(),0);
            release.note_off(0,60);
            release.render(&mut left,&mut right);
            assert_eq!(release.active_voices(),1,"release sample owns an independent pending onset");
        }),0);
        release_shared.paused.store(false,Ordering::Release); wake(&release_shared);
        release.blocking_streams = true;
        assert_eq!(crate::plugin::tests::allocations(|| {
            release.render_checked(&mut left,&mut right).unwrap();
            assert!(left.iter().chain(&right).any(|v| v.abs() > 0.01));
        }),0);
    }

    #[test]
    fn offline_decode_failure_is_explicit_and_genuine_zero_pcm_is_successful() {
        use crate::engine::{Bank, Engine};
        let dir = tempfile::tempdir().unwrap();
        for silent in [false,true] {
            let path = dir.path().join(if silent { "zero.wav" } else { "deleted.wav" });
            let source = instrument(&path,silent);
            let bank = Bank::load_bare(&source).unwrap();
            if !silent { std::fs::remove_file(&path).unwrap(); }
            let mut engine = Engine { blocking_streams: true, ..Default::default() };
            engine.set_bank(Some(Box::new(bank)));
            let (mut left,mut right) = ([1.;128],[1.;128]);
            let mut outcome = Ok(());
            assert_eq!(crate::plugin::tests::allocations(|| {
                engine.note_on(0,60,127);
                outcome = engine.render_checked(&mut left,&mut right);
            }),0);
            assert_eq!((left,right),([0.;128],[0.;128]));
            if silent { assert_eq!(outcome,Ok(())); assert_eq!(engine.active_voices(),1); }
            else {
                assert_eq!(outcome,Err(StreamError { sample:0,frame:0,kind:StreamErrorKind::Decode }));
                assert_eq!(engine.active_voices(),0,"failed pending voice is retired");
            }
        }
    }

    #[test]
    fn readiness_failure_is_generation_scoped_and_timeout_never_claims_success() {
        let ring: Vec<_> = (0..RING).map(|_| AtomicU64::new(0)).collect();
        let mut wake = Wake::default();
        let slot = Slot::new(NonNull::from(&ring[0]),NonNull::from(&mut wake));
        let map = PlayMap { start:0,end:4096,reverse:false,looped:None };
        let old = slot.configure(0,&map,0,0,0);
        assert_eq!(slot.ready(old,128,false),Ok(false));
        assert_eq!(slot.wait_ready(old,128,true,Duration::ZERO),Err(StreamErrorKind::Timeout));
        slot.failed_at.store(1024,Ordering::Relaxed);
        slot.failed.store(u64::from(slot.seq.load(Ordering::Acquire)) << 32 | 1,Ordering::Release);
        slot.written.store(u64::from(old) << 48 | 1024,Ordering::Release);
        slot.dense_published_seq.store(slot.seq.load(Ordering::Acquire), Ordering::Release);
        assert_eq!(slot.ready(old,128,true),Ok(true),"valid prefix remains readable");
        assert_eq!(slot.ready(old,2048,true),Err(StreamErrorKind::Decode));
        slot.stop();
        assert_eq!(slot.ready(old,128,true),Err(StreamErrorKind::Canceled));
        let new = slot.configure(0,&map,0,0,0);
        assert_eq!(slot.ready(new,128,false),Ok(false),"old failure cannot poison the reused slot");
        slot.written.store(u64::from(new) << 48 | 128,Ordering::Release);
        slot.dense_published_seq.store(slot.seq.load(Ordering::Acquire), Ordering::Release);
        assert_eq!(slot.ready(new,128,true),Ok(true));
    }

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
    fn sparse_packets_skip_gaps_retarget_and_rebind_without_audio_heap_operations() {
        let dir = tempfile::tempdir().unwrap();
        let instrument = instrument(&dir.path().join("sparse.wav"), false);
        let pcm = crate::audio::decode(&instrument.zones[0].sample, 131072).unwrap();
        let mut sources = crate::audio::Sources::default();
        let source = Arc::new(sources.source(&instrument.zones[0].sample).unwrap());
        let sources = [Some(source), None];
        let ring: Vec<_> = (0..RING).map(|_| AtomicU64::new(0)).collect();
        let mut wake = Wake::default();
        let slot = Slot::new(NonNull::from(&ring[0]), NonNull::from(&mut wake));
        let map = PlayMap { start:0, end:131072, reverse:false, looped:None };
        let generation = slot.configure(0, &map, 0, 0, 0);
        let mut taps: [u64; SPARSE_TAPS] = std::array::from_fn(|i| (i / 8 * 1024 + i % 4) as u64);
        taps[0] = u64::MAX;
        let mut output = [[0.; 2]; SPARSE_TAPS];
        let mut old = None;
        let mut request = None;
        assert_eq!(crate::plugin::tests::allocations(|| {
            old = Some(slot.request_sparse(generation, &taps).unwrap());
            for _ in 0..16 {
                assert_eq!(slot.request_sparse(generation, &taps), Ok(old.unwrap()));
                assert_eq!(slot.ready_sparse(generation, old.unwrap(), false), Ok(false));
            }
            assert_eq!(slot.wait_sparse(generation, old.unwrap(), true, Duration::ZERO), Err(StreamErrorKind::Timeout));
            taps[1] = 8192;
            request = Some(slot.request_sparse(generation, &taps).unwrap());
            assert_ne!(request, old);
            assert_eq!(slot.ready_sparse(generation, old.unwrap(), false), Err(StreamErrorKind::Canceled));
        }), 0);
        let (old, request) = (old.unwrap(), request.unwrap());
        let (mut worker, mut cursor, rings) = (Worker::new(), Cursor::default(), RingUse::default());
        assert!(worker.serve(&slot, &mut cursor, URGENT, &rings, &sources));
        assert_eq!(worker.decoded.decodes, 64, "only blocks containing requested taps were decoded");
        assert_eq!(cursor.next, 0, "sparse filling never traverses dense skipped source frames");
        assert!(!worker.serve(&slot, &mut cursor, RING, &rings, &sources), "completed packet stays immutable");
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert_eq!(slot.ready_sparse(generation, request, false), Ok(true));
            assert!(!slot.copy_sparse(generation, old, &mut output));
            assert!(slot.copy_sparse(generation, request, &mut output));
        }), 0);
        for (tap, frame) in taps.iter().zip(&output) {
            assert_eq!(*frame, if *tap == u64::MAX { [0.; 2] } else { pcm.frames[*tap as usize] });
        }
        // Stop, reuse, and a different source all invalidate the old packet,
        // even when the new request epoch and tap list happen to be identical.
        slot.stop();
        assert_eq!(slot.ready_sparse(generation, request, false), Err(StreamErrorKind::Canceled));
        let missing = slot.configure(1, &map, 0, 0, 0);
        let missing_request = slot.request_sparse(missing, &taps).unwrap();
        assert_eq!(slot.ready_sparse(missing, missing_request, false), Ok(false));
        assert!(!slot.copy_sparse(missing, missing_request, &mut output));
        worker.serve(&slot, &mut cursor, URGENT, &rings, &sources);
        assert_eq!(slot.ready_sparse(missing, missing_request, false), Err(StreamErrorKind::Unavailable));
        assert!(!slot.copy_sparse(missing, missing_request, &mut output));
        let rebound = slot.configure(0, &map, 0, 0, 0);
        let zero = [u64::MAX; SPARSE_TAPS];
        let rebound_request = slot.request_sparse(rebound, &zero).unwrap();
        assert_eq!(slot.ready_sparse(rebound, rebound_request, false), Ok(false));
        worker.serve(&slot, &mut cursor, URGENT, &rings, &sources);
        assert_eq!(slot.ready_sparse(rebound, rebound_request, false), Ok(true));
        assert!(slot.copy_sparse(rebound, rebound_request, &mut output));
        assert_eq!(output, [[0.; 2]; SPARSE_TAPS], "genuine zero PCM is successful");
        slot.sparse_seq.store(u32::MAX - 1, Ordering::Release);
        assert_eq!(slot.request_sparse(rebound, &taps), Err(StreamErrorKind::Unavailable), "request epochs never alias within a live configuration");
    }

    #[test]
    fn sparse_linear_matching_keeps_earliest_exact_floor_and_padding_matches_without_heap() {
        let values: [u64; SPARSE_TAPS] = std::array::from_fn(|i| {
            let frame = i / 4;
            if frame >= 192 { u64::MAX }
            else { [if frame < 128 { 0 } else { (frame % 3) as u64 }, 1, u64::MAX, 3][i % 4] }
        });
        let saved = std::array::from_fn(|i| AtomicU64::new(values[i]));
        let mut taps = [0; SPARSE_TAPS];
        assert_eq!(crate::plugin::tests::allocations(|| {
            for start in [0, 1, 7, 63, 128, 192, 255] {
                for frames in [1, 2, 7, 32, 128, 256] {
                    if start + frames > SPARSE_TAPS / 4 { continue; }
                    let len = frames * 4;
                    taps[..len].copy_from_slice(&values[start * 4..start * 4 + len]);
                    let expected = (0..=SPARSE_TAPS - len).step_by(4)
                        .find(|offset| values[*offset..*offset + len] == taps[..len]);
                    assert_eq!(sparse_offset(&taps[..len], &saved, SPARSE_TAPS), expected);
                    // A mismatch after a long repeated prefix must remain exact.
                    taps[len - 2] = 42;
                    assert_eq!(sparse_offset(&taps[..len], &saved, SPARSE_TAPS), None);
                }
            }
        }), 0);
    }

    #[test]
    fn sparse_realtime_lookahead_continues_after_onset_across_short_and_full_callbacks() {
        use crate::engine::{Bank, Engine};
        let dir = tempfile::tempdir().unwrap();
        let mut instrument = instrument(&dir.path().join("wide-live.wav"), false);
        instrument.zones[0].tune = 72.;
        instrument.groups[0].mods.push(crate::import::ModAssignment { cc_default: None,
            name:"Live wide pitch".into(), source:crate::import::ModSource::MidiCc(1),
            target:crate::import::ModTarget::Pitch, intensity:1., lag_ms:0,
            invert:false, shaper:None,
        });
        let pcm = crate::audio::decode(&instrument.zones[0].sample, 131072).unwrap();
        let bank = Bank::load_bare(&instrument).unwrap();
        let shared = pause(&bank);
        let sources = bank.streamer.as_ref().unwrap().sources();
        let ram = Bank::from_samples(instrument.groups.clone(), instrument.zones.clone(),
            vec![(instrument.zones[0].sample.clone(), pcm)]).unwrap();
        let (mut actual, mut oracle) = (Engine::default(), Engine::default());
        actual.attack = 0.; oracle.attack = 0.;
        actual.set_bank(Some(Box::new(bank))); oracle.set_bank(Some(Box::new(ram)));
        actual.note_on(0, 60, 127); oracle.note_on(0, 60, 127);
        let stream = actual.player.voices[0].stream.unwrap();
        let slot = &shared.slots[stream.slot as usize];
        let (mut worker, mut cursor) = (Worker::new(), Cursor::default());
        let (mut left, mut right, mut expected_l, mut expected_r) = ([0.;128], [0.;128], [0.;128], [0.;128]);
        assert_eq!(crate::plugin::tests::allocations(|| actual.render_checked(&mut left, &mut right).unwrap()), 0);
        assert_eq!(left, [0.;128]);
        assert_eq!(actual.player.voices[0].pos, 0.);
        assert!(actual.player.voices[0].onset == super::super::voice::Onset::Waiting);
        // No background races: serve the exact pending packet and every
        // subsequent lookahead between callbacks, while realtime stays active.
        let mut total = 0;
        for n in [128, 1, 7, 31, 128, 1, 7, 31, 128, 128, 1, 7, 128] {
            worker.serve(slot, &mut cursor, URGENT, &shared.rings, &sources);
            assert_eq!(crate::plugin::tests::allocations(|| {
                actual.render_checked(&mut left[..n], &mut right[..n]).unwrap();
                oracle.render_checked(&mut expected_l[..n], &mut expected_r[..n]).unwrap();
            }), 0);
            total += n;
            assert_eq!(&left[..n], &expected_l[..n], "realtime left after {total} output frames");
            assert_eq!(&right[..n], &expected_r[..n], "realtime right after {total} output frames");
            assert!(left[..n].iter().chain(&right[..n]).any(|v| v.abs() > 0.001), "prepared lookahead must produce genuine PCM after onset");
            assert_eq!(actual.underruns(), 0, "every needed packet was prepared before this callback");
        }
        assert!(total > 128 * 3);
        assert!(!actual.blocking_streams, "this proof exercises realtime packet reuse");
        // A changed pitch must reject old cached taps. The established clock
        // still advances, while its newly previewed trajectory prepares recovery.
        for engine in [&mut actual, &mut oracle] { engine.cc(0, 1, 127); }
        let before = actual.player.voices[0].pos;
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.render_checked(&mut left, &mut right).unwrap();
            oracle.render_checked(&mut expected_l, &mut expected_r).unwrap();
        }), 0);
        assert_eq!(left, [0.;128], "a changed trajectory cannot play the old cached positions");
        assert!(actual.player.voices[0].pos > before, "established source clocks keep advancing through a truthful underrun");
        worker.serve(slot, &mut cursor, URGENT, &shared.rings, &sources);
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.render_checked(&mut left, &mut right).unwrap();
            oracle.render_checked(&mut expected_l, &mut expected_r).unwrap();
        }), 0);
        assert_eq!(left, expected_l, "prepared retuned lookahead restores realtime PCM");
        assert_eq!(right, expected_r);
        let before_reset = actual.player.voices[0].stream.unwrap();
        actual.reset(48_000.);
        assert_eq!(slot.ready_sparse(before_reset.tag, before_reset.sparse.unwrap(), false), Err(StreamErrorKind::Canceled));
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
    fn failed_decoder_retains_live_ring_pcm_until_stopped_generation_reclaims_without_heap() {
        use crate::engine::{Bank, Engine, HostNote, HostPattern};
        const OFF: usize = 240_001;
        const END: usize = OFF + 1024;
        let _diagnostics = crate::diagnostics::acquire();
        let dir = tempfile::tempdir().unwrap();
        let mut instrument = instrument(&dir.path().join("retained-ring.wav"), false);
        instrument.zones[0].tune = 2f64.powf(-120. / 12.);
        let decoded = crate::audio::decode(&instrument.zones[0].sample, 131072).unwrap();
        let bad_path = dir.path().join("broken-ahead.wav");
        std::fs::write(&bad_path, [0u8; 256]).unwrap();
        let broken = Arc::new(crate::audio::Sources::default().source(&bad_path).unwrap());
        let owner = HostNote { port:0, channel:0, key:60, id:911, clap:true };
        let pattern = HostPattern { port:0, channel:0, key:60, id:911, clap:true };
        let mut performances = Vec::new();
        for fragments in [&[128][..], &[1, 7, 31, 127][..]] {
            let bank = Bank::load_bare(&instrument).unwrap();
            let shared = pause(&bank);
            let sources = bank.streamer.as_ref().unwrap().sources();
            let ram = Bank::from_samples(instrument.groups.clone(), instrument.zones.clone(),
                vec![(instrument.zones[0].sample.clone(), crate::audio::Sample {
                    rate:decoded.rate, frames:decoded.frames.clone() })]).unwrap();
            let (mut actual, mut expected) = (Engine::default(), Engine::default());
            for engine in [&mut actual, &mut expected] { engine.attack=0.; engine.release=0.005; }
            actual.set_bank(Some(Box::new(bank))); expected.set_bank(Some(Box::new(ram)));
            for engine in [&mut actual, &mut expected] { assert!(engine.host_note_on(owner,0,60,127,0.)); }
            let first = actual.player.voices[0].limit + 16;
            for engine in [&mut actual, &mut expected] { engine.player.voices[0].pos = first as f64; }
            let voice = &mut actual.player.voices[0];
            let stream = voice.stream.as_mut().unwrap();
            let slot = &shared.slots[stream.slot as usize];
            stream.tag = slot.configure(voice.sample, &voice.map, voice.wraps, voice.limit, first-1);
            stream.trusted = voice.limit;
            let generation = stream.tag;
            let (mut worker, mut cursor) = (Worker::new(), Cursor::default());
            assert!(worker.serve(slot, &mut cursor, URGENT, &shared.rings, &sources));
            let published = slot.published(generation).unwrap();
            assert!(published > first + 256);
            // A later speculative chunk really fails decoding. The old chunk
            // stays valid; failed worker work is not an audio stop command.
            assert!(worker.serve(slot, &mut cursor, RING, &shared.rings, &[Some(broken.clone())]));
            assert!(cursor.config.is_none());
            assert_eq!(slot.failed_at.load(Ordering::Relaxed), published);
            assert_eq!(slot.failed.load(Ordering::Acquire) as u32, 1);
            assert!(cursor.pages == Pages::Touched);
            assert_eq!(slot.ready(generation, first+4, false), Ok(true));
            assert_eq!(slot.ready(generation, published+1, false), Err(StreamErrorKind::Decode));
            let mut pcm: [[Vec<f32>; 2]; 2] = std::array::from_fn(|_| std::array::from_fn(|_| vec![0.; END]));
            assert_eq!(crate::plugin::tests::allocations(|| {
                // This is the exact reclamation operation used after the idle
                // timeout, called deterministically by the sole fixture worker.
                assert!(!slot.reclaim(cursor.seq), "live failure publication must retain its pages");
                let (mut at, mut fragment) = (0, 0);
                for stop in [OFF, END] {
                    while at < stop {
                        let n = fragments[fragment % fragments.len()].min(stop-at);
                        for (engine, output) in [&mut actual, &mut expected].into_iter().zip(&mut pcm) {
                            let [left,right] = output;
                            engine.render_checked(&mut left[at..at+n], &mut right[at..at+n]).unwrap();
                        }
                        at += n; fragment += 1;
                    }
                    if stop == OFF {
                        assert!(!slot.reclaim(cursor.seq));
                        assert!(actual.player.voices[0].pos < published as f64 - 4.);
                        assert_eq!(actual.underruns(), 0);
                        for engine in [&mut actual, &mut expected] { engine.host_note_off(pattern); }
                    }
                }
                for engine in [&mut actual, &mut expected] {
                    engine.mark_host_notes();
                    assert_eq!(engine.active_voices(), 0);
                    assert_eq!(engine.underruns(), 0);
                }
            }), 0);
            assert_eq!(actual.host_note_end_at(owner), expected.host_note_end_at(owner));
            assert!(actual.host_note_end_at(owner).is_some_and(|at| at > OFF as u64 && at < END as u64));
            assert_eq!(pcm[0], pcm[1], "published ring PCM must match independent resident source through release");
            assert!(pcm[0][0][OFF-128..OFF].iter().any(|value| value.abs() > 0.001));
            assert!(pcm[0].iter().all(|channel| channel[END-128..].iter().all(|value| *value == 0.)));
            assert!(!slot.reclaim(cursor.seq), "the failed active generation cannot reclaim a later stop");
            let stopped = slot.seq.load(Ordering::Acquire);
            assert_eq!(slot.sample.load(Ordering::Relaxed), NO_SAMPLE);
            #[cfg(target_os="linux")]
            assert!(slot.reclaim(stopped), "the observed stopped generation owns reclamation");
            let map = PlayMap { start:0, end:131072, reverse:false, looped:None };
            let restarted = slot.configure(0, &map, 0, first-1, first-1);
            assert!(!slot.reclaim(stopped), "a restarted slot rejects stale reclamation ownership");
            assert_eq!(slot.ready(restarted, first+4, false), Ok(false));
            assert!(worker.serve(slot, &mut cursor, URGENT, &shared.rings, &sources));
            assert_eq!(slot.ready(restarted, first+4, false), Ok(true));
            let mut fresh = [[0.;2];4];
            slot.copy(first, &mut fresh);
            assert_eq!(fresh, decoded.frames[first as usize..first as usize+4]);
            slot.stop();
            performances.push(pcm);
        }
        let error = performances[0].iter().flatten().flatten().zip(performances[1].iter().flatten().flatten())
            .fold(0_f32, |error,(a,b)| error.max((a-b).abs()));
        assert!(error < 2e-5, "host fragments changed retained-ring final PCM by {error}");
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
        let read = |worker: &mut Worker, source: &Arc<Source>| worker.decoded.read_run(Some(source), &run,
            &mut worker.frames[..64], &mut worker.partners[..64]);
        worker.frames[..64].fill([1.0; 2]);
        assert_eq!(read(&mut worker, &sources[0]),Err(StreamErrorKind::Decode));
        assert!(worker.frames[..64].iter().all(|f| *f == [0.0; 2]));
        let first_id = Arc::as_ptr(&sources[0]) as usize;
        let first = worker.decoded.errors[&first_id].1.unwrap();
        assert_eq!(read(&mut worker, &sources[0]),Err(StreamErrorKind::Decode));
        assert_eq!(worker.decoded.errors[&first_id].1, Some(first), "repeated source does not format/log again within one second");
        assert_eq!(read(&mut worker, &sources[1]),Err(StreamErrorKind::Decode));
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
        assert_eq!(read(&mut worker, &sources[0]),Err(StreamErrorKind::Decode));
        let snapshot = serde_json::to_value(crate::diagnostics::snapshot()).unwrap();
        assert!(snapshot["events"].as_array().unwrap().iter().any(|event|
            event["code"] == "decode_failed" && event["path"] == paths[0].to_string_lossy().as_ref()
                && event["data"]["source_decode_failures"] == 3 && event["data"]["decode_failures"] == 4),
            "the next summary includes all suppressed failures");
        let retiring: Vec<_> = (0..READERS + 2).map(|_| Arc::new(crate::audio::Sources::default().source(&paths[1]).unwrap())).collect();
        for source in &retiring { assert_eq!(read(&mut worker, source),Err(StreamErrorKind::Decode)); }
        assert_eq!(worker.decoded.errors.len(), READERS, "bookkeeping never grows past the existing reader-cache ceiling");
        drop(retiring);
        assert_eq!(read(&mut worker, &sources[0]),Err(StreamErrorKind::Decode));
        assert!(worker.decoded.errors.len() <= sources.len());
        assert!(!worker.decoded.errors.values().any(|(source, _, _)| source.strong_count() == 0), "retired source identities are pruned before reuse");
        worker.decoded.clear();
        assert!(worker.decoded.errors.is_empty());
        drop(sources);
        for path in paths { std::fs::remove_file(path).unwrap(); }
    }
}
