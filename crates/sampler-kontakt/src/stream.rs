//! Streamed instruments: v1 resident start spans and source-time voice rings.

use crate::{
    LoadError,
    samples::{FileAt, Source},
};
use sampler_core::{AssetId, DecodeFailure, PAGE_FRAMES, Pcm, StreamCache, StreamWorker};
use sampler_ir as ir;
mod preload;
mod decoded;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

type Frame = [f32; 2];

/// A sample's bytes as a seekable stream, decrypted on the fly. Reads go
/// through one buffer: decoders read a few bytes at a time.
/// Sample bytes streamed from disk so far (loading reads are not counted).
/// Port from v1 0cb7a8a0:src/audio.rs.
pub static DISK_READ: AtomicU64 = AtomicU64::new(0);

struct Bytes {
    counted: bool,
    file: Arc<File>,
    source: Source,
    position: u64,
    /// Decrypted bytes from member offset `at`.
    buffer: Vec<u8>,
    at: u64,
}

/// Bytes per file read.
const BUFFER: usize = 16 << 10;

impl Bytes {
    fn open(source: &Source, counted: bool) -> io::Result<Self> {
        Ok(Self {
            counted,
            file: match &source.handle {
                Some(handle) => handle.clone(),
                None => Arc::new(File::open(&source.path)?),
            },
            source: source.clone(),
            position: 0,
            buffer: Vec::with_capacity(BUFFER),
            at: 0,
        })
    }
}

impl Read for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let end = self.at + self.buffer.len() as u64;
        if self.position < self.at || self.position >= end {
            let left = self.source.size.saturating_sub(self.position);
            let len = BUFFER.min(usize::try_from(left).unwrap_or(usize::MAX));
            let mut file = FileAt {
                file: &self.file,
                pos: self.source.offset + self.position,
            };
            self.buffer.resize(len, 0);
            let mut filled = 0;
            while filled < len {
                match file.read(&mut self.buffer[filled..])? {
                    0 => break,
                    n => { filled += n; if self.counted { DISK_READ.fetch_add(n as u64, Ordering::Relaxed); } },
                }
            }
            self.buffer.truncate(filled);
            if let Some(key) = &self.source.key {
                key.apply_at(self.position, &mut self.buffer);
            }
            self.at = self.position;
        }
        let offset = (self.position - self.at) as usize;
        let n = buf.len().min(self.buffer.len() - offset);
        buf[..n].copy_from_slice(&self.buffer[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for Bytes {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.position = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.source.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.position)
    }
}

impl symphonia::core::io::MediaSource for Bytes {
    fn is_seekable(&self) -> bool { true }
    fn byte_len(&self) -> Option<u64> { Some(self.source.size) }
}

/// Fills frames from a start.
type Reader = Box<dyn FnMut(usize, &mut [Frame]) -> io::Result<()> + Send>;

enum Codec {
    Wav(crate::samples::Wav),
    Ncw(Box<ncw::NcwReader<Bytes>>),
    /// A reader supplied by another loader: fills frames from a start.
    Custom(Reader, usize),
}

/// Where an asset's frames are read from, reopened by each decode thread.
pub trait AssetSource: Send + Sync {
    fn open(&self) -> io::Result<SampleReader>;
    /// A validated persistent numeric header, when available.
    fn header(&self) -> Option<(u32, usize)> {
        None
    }
    /// Maximum packed stereo bytes per source frame, for control-side preload planning.
    fn frame_bytes(&self) -> Option<usize> {
        None
    }
    /// Playback/reload IO, counted separately from initial loading.
    fn open_stream(&self) -> io::Result<SampleReader> { self.open() }
}

impl AssetSource for Source {
    fn open(&self) -> io::Result<SampleReader> {
        SampleReader::open(self)
    }
    fn header(&self) -> Option<(u32, usize)> {
        self.header
    }
    fn frame_bytes(&self) -> Option<usize> {
        self.frame_bytes
    }
    fn open_stream(&self) -> io::Result<SampleReader> {
        SampleReader::open_counted(self, true)
    }
}

/// Random-access frames of one sample, converted exactly as a full decode.
pub struct SampleReader {
    codec: Codec,
    /// The WAV byte stream (NCW keeps its own inside the decoder).
    bytes: Option<Bytes>,
    rate: u32,
    frames: usize,
    scratch: Vec<u8>,
}

impl SampleReader {
    pub fn open(source: &Source) -> io::Result<Self> { Self::open_counted(source, false) }

    fn open_counted(source: &Source, counted: bool) -> io::Result<Self> {
        let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        let mut bytes = Bytes::open(source, counted)?;
        let mut head = Vec::new();
        (&mut bytes).take(1 << 12).read_to_end(&mut head)?;
        if head.starts_with(b"RIFF") && crate::samples::wav_layout(&head).is_err() {
            // A WAV header past 4 KiB: read up to 64 KiB.
            bytes.seek(SeekFrom::Start(0))?;
            head.clear();
            (&mut bytes).take(1 << 16).read_to_end(&mut head)?;
        }
        if head.starts_with(b"RIFF") {
            let wav = crate::samples::wav_layout(&head).map_err(invalid)?;
            let align = wav.width * wav.channels;
            let end = wav
                .data
                .end
                .min(usize::try_from(source.size).unwrap_or(usize::MAX));
            let frames = end.saturating_sub(wav.data.start) / align;
            return Ok(Self {
                rate: wav.rate,
                frames,
                codec: Codec::Wav(wav),
                bytes: Some(bytes),
                scratch: Vec::new(),
            });
        }
        bytes.seek(SeekFrom::Start(0))?;
        if head.starts_with(b"FORM") {
            let mut reader = crate::pcm::Reader::open(Box::new(bytes)).map_err(|e| invalid(format!("AIFF: {e:#}")))?;
            let frames = usize::try_from(reader.frames).map_err(|_| invalid("AIFF too long".into()))?;
            let width = reader.frame_bytes;
            let mut sample = Self::custom(reader.rate, frames, move |start, out| reader.read(start as u64, out).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)));
            if let Codec::Custom(_, native) = &mut sample.codec { *native = width; }
            return Ok(sample);
        }
        let reader = ncw::NcwReader::read(bytes).map_err(|e| invalid(format!("NCW: {e}")))?;
        Ok(Self {
            rate: reader.header.sample_rate,
            frames: reader.header.num_samples as usize,
            codec: Codec::Ncw(Box::new(reader)),
            bytes: None,
            scratch: Vec::new(),
        })
    }

    /// A reader of `frames` frames at `rate` that fills output from `read(start, out)`.
    pub fn custom(
        rate: u32,
        frames: usize,
        read: impl FnMut(usize, &mut [Frame]) -> io::Result<()> + Send + 'static,
    ) -> Self {
        Self {
            codec: Codec::Custom(Box::new(read), 8),
            bytes: None,
            rate,
            frames,
            scratch: Vec::new(),
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
    pub fn frames(&self) -> usize {
        self.frames
    }
    fn frame_bytes(&self) -> usize {
        let bits = match &self.codec {
            Codec::Wav(wav) => Some(wav.width * 8),
            Codec::Ncw(ncw) if ncw.sample_format != ncw::SampleFormat::Float => {
                Some(ncw.header.bits_per_sample as usize)
            }
            Codec::Custom(_, width) => return *width,
            _ => None,
        };
        match bits {
            Some(..=16) => 4,
            Some(..=24) => 6,
            _ => 8,
        }
    }

    /// Fill `out` with frames from `start`.
    pub fn read(&mut self, start: usize, out: &mut [Frame]) -> io::Result<()> {
        let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        if start + out.len() > self.frames {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        match &mut self.codec {
            Codec::Custom(read, _) => read(start, out)?,
            Codec::Wav(wav) => {
                let align = wav.width * wav.channels;
                let bytes = self.bytes.as_mut().expect("WAV bytes");
                bytes.seek(SeekFrom::Start((wav.data.start + start * align) as u64))?;
                self.scratch.resize(out.len() * align, 0);
                bytes.read_exact(&mut self.scratch)?;
                for (frame, data) in out.iter_mut().zip(self.scratch.chunks_exact(align)) {
                    *frame = wav.frame(data);
                }
            }
            Codec::Ncw(reader) => {
                const BLOCK: usize = ncw::NcwReader::<Bytes>::FRAMES_PER_BLOCK;
                let convert =
                    crate::samples::ncw_sample(reader.sample_format, reader.header.bits_per_sample);
                let last = reader.header.channels.min(2) as usize - 1;
                let mut at = start;
                let mut out = &mut out[..];
                while !out.is_empty() {
                    let channels = reader
                        .decode_block(at / BLOCK)
                        .map_err(|e| invalid(format!("NCW: {e}")))?;
                    let offset = at % BLOCK;
                    let n = (channels[0].len().saturating_sub(offset)).min(out.len());
                    if n == 0 {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    for (i, frame) in out[..n].iter_mut().enumerate() {
                        *frame = [
                            convert(channels[0][offset + i]),
                            convert(channels[last][offset + i]),
                        ];
                    }
                    at += n;
                    out = &mut out[n..];
                }
            }
        }
        Ok(())
    }
}

/// How streamed assets were sized, and what is resident.
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamReport {
    /// Head read latency over the probed assets, open plus first page.
    pub latency_p50: Duration,
    pub latency_p95: Duration,
    pub latency_p99: Duration,
    /// Source frames resident beyond each preloaded start, fitted to the budget.
    pub head_frames: usize,
    pub head_bytes: usize,
    /// Bytes every asset would take fully decoded, as `load` holds them.
    pub full_bytes: u64,
    pub pool_pages: usize,
    pub pool_bytes: usize,
}

/// How much to keep resident.
#[derive(Clone, Copy, Debug)]
pub struct StreamPolicy {
    /// Pitch a voice may rise above its zone's highest key, in semitones
    /// (bend and modulation headroom), for head sizing.
    pub headroom: f64,
    /// Ceiling on any zone's estimated step.
    pub max_step: f64,
    /// Worst wait between a page request and its decode starting (worker poll
    /// and the host's service period), on top of measured read latency.
    pub slack: Duration,
    /// The host's largest render block, in output frames. A voice that starts
    /// inside a block is first serviced before the next one, so its head must
    /// also cover that whole block before its next page can even be requested.
    pub block_frames: usize,
    /// Voices that can stream at once; each owns an 8192-frame source-time ring.
    pub voices: usize,
    /// Decode threads.
    pub decoders: usize,
    /// Total voice-ring and resident start-range budget in bytes.
    pub head_budget: usize,
    /// Publish before heads are read. Runtime must enable cold-start holding.
    pub lazy: bool,
    /// RAM-only: keep whole samples smallest first within this safe byte budget.
    pub resident_budget: Option<usize>,
}

impl Default for StreamPolicy {
    fn default() -> Self {
        Self {
            headroom: 2.0,
            max_step: 4.0,
            slack: Duration::from_millis(10),
            block_frames: 64,
            voices: 1024,
            decoders: 4,
            head_budget: 128 << 20,
            lazy: false,
            resident_budget: None,
        }
    }
}

/// Pool pages per streaming voice: the page it reads, the next, and one more
/// for a window or crossfade spanning a page boundary.
const PAGES_PER_VOICE: usize = 3;

/// Output frames a start plays before the page after its head can arrive:
/// the wait from the first service after the start (which requests that page,
/// given a service horizon of at least the head) to its decode.
pub(crate) fn head_frames(latency: Duration, rate: u32, policy: &StreamPolicy) -> usize {
    ((latency + policy.slack).as_secs_f64() * f64::from(rate)).ceil() as usize + policy.block_frames
}

// Port from v1 0cb7a8a0:src/engine/bank.rs (Builder::keep_whole).
/// Load samples whole instead of streaming them, smallest first, while
/// they fit `room` bytes. Returns the bytes the rest would need.
fn keep_whole(assets: &[Pcm], ranges: &mut [Vec<Range<usize>>], frame_bytes: &[usize], room: usize) -> usize {
    assert_eq!(assets.len(), frame_bytes.len());
    let size = |i: usize| assets[i].frame_count().saturating_mul(frame_bytes[i]);
    let resident = |i: usize, list: &[Range<usize>]| list.iter().map(|r| r.end - r.start).sum::<usize>() * frame_bytes[i];
    let mut streamed: Vec<_> = (0..ranges.len()).filter(|&i| !ranges[i].is_empty() && resident(i, &ranges[i]) < size(i)).collect();
    streamed.sort_by_key(|&i| size(i));
    let bytes: usize = ranges.iter().enumerate().map(|(i,r)| resident(i,r)).sum();
    let mut room = room.saturating_sub(bytes);
    let mut needed = 0;
    for i in streamed {
        let more = size(i).saturating_sub(resident(i, &ranges[i]));
        if more <= room {
            room -= more;
            ranges[i] = std::iter::once(0..assets[i].frame_count()).collect();
        } else {
            needed += more - room.min(more);
            room = 0;
        }
    }
    needed
}


/// Streamed assets before their start ranges are read.
pub(crate) struct Opened {
    pub assets: Vec<Pcm>,
    pub frame_bytes: Vec<usize>,
    pub sources: HashMap<AssetId, Arc<dyn AssetSource>>,
    pub report: StreamReport,
    pub policy: StreamPolicy,
}

/// Read `ranges` of `pcm` from `reader` and make them resident; their bytes.
fn load_ranges(
    pcm: &Pcm,
    reader: &mut SampleReader,
    ranges: &[Range<usize>],
    compress: bool,
) -> io::Result<usize> {
    let mut resident = Vec::with_capacity(ranges.len());
    for range in ranges {
        let mut frames = vec![[0.0; 2]; range.len()];
        reader.read(range.start, &mut frames)?;
        resident.push((range.start, frames.into_boxed_slice()));
    }
    pcm.set_ranges_with_compression(resident, compress)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(pcm.head_bytes())
}

/// Control side: where every streamed asset lives, its start ranges, and the
/// decode threads. Purge and reload start ranges here; the runtime owns voice rings.
pub struct Streamer {
    sources: Arc<HashMap<AssetId, Arc<dyn AssetSource>>>,
    ranges: Arc<HashMap<AssetId, (Vec<Range<usize>>, bool)>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    reloader: Option<JoinHandle<()>>,
    head_budget: usize,
}

impl Streamer {
    /// Clone the existing source for bounded control-worker reads. Opening or
    /// decoding it belongs off the audio thread; this does not preload frames.
    pub fn source(&self, asset: AssetId) -> Option<Arc<dyn AssetSource>> {
        self.sources.get(&asset).cloned()
    }

    /// Open every source as a streamed asset, timing an open plus first-page
    /// read on up to `probe` of them to size heads by `policy`.
    pub(crate) fn open(
        sources: Vec<(Arc<dyn AssetSource>, &std::path::Path)>,
        rate: u32,
        policy: &StreamPolicy,
        probe: usize,
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Opened, LoadError> {
        let _span = crate::audit::Span::new("sample_headers_latency_probe");
        let opened = std::thread::scope(|scope| {
            let sources = &sources;
            let count = std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(8)
                .min(sources.len());
            let workers: Vec<_> = (0..count)
                .map(|worker| {
                    scope.spawn(move || {
                        let mut page = vec![[0.0; 2]; PAGE_FRAMES];
                        (worker..sources.len())
                            .step_by(count)
                            .map(|i| {
                                let (source, path) = &sources[i];
                                let result = (|| {
                                    if canceled() {
                                        return Err(LoadError::Canceled);
                                    }
                                    if let (Some((rate, frames)), Some(width)) =
                                        (source.header(), source.frame_bytes())
                                    {
                                        if !matches!(width, 4 | 6 | 8) {
                                            return Err(LoadError::Invalid { path:(*path).into(), reason:"invalid native frame width".into() });
                                        }
                                        let pcm = Pcm::streamed(rate, frames).map_err(|e| {
                                            LoadError::Invalid {
                                                path: (*path).into(),
                                                reason: e.to_string(),
                                            }
                                        })?;
                                        return Ok((pcm, None, width));
                                    }
                                    let begin = Instant::now();
                                    let mut reader =
                                        source.open().map_err(|e| LoadError::io(path, e))?;
                                    let latency = if i < probe {
                                        let n = reader.frames().min(PAGE_FRAMES);
                                        reader
                                            .read(0, &mut page[..n])
                                            .map_err(|e| LoadError::io(path, e))?;
                                        Some(begin.elapsed())
                                    } else {
                                        None
                                    };
                                    let pcm = Pcm::streamed(reader.rate(), reader.frames())
                                        .map_err(|e| LoadError::Invalid {
                                            path: (*path).into(),
                                            reason: e.to_string(),
                                        })?;
                                    Ok((pcm, latency, reader.frame_bytes()))
                                })();
                                (i, result)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut opened: Vec<_> = workers
                .into_iter()
                .flat_map(|w| w.join().expect("sample header worker"))
                .collect();
            opened.sort_unstable_by_key(|(i, _)| *i);
            opened
                .into_iter()
                .map(|(_, result)| result)
                .collect::<Result<Vec<_>, LoadError>>()
        })?;
        let mut latencies = Vec::new();
        let mut assets = Vec::with_capacity(sources.len());
        let mut frame_bytes = Vec::with_capacity(sources.len());
        let mut registry = HashMap::with_capacity(sources.len());
        let mut full = 0;
        for ((source, _), (pcm, latency, width)) in sources.into_iter().zip(opened) {
            if let Some(latency) = latency {
                latencies.push(latency);
            }
            full += pcm.frame_count() as u64 * size_of::<Frame>() as u64;
            registry.insert(pcm.asset_id(), source);
            assets.push(pcm);
            frame_bytes.push(width);
        }
        latencies.sort_unstable();
        let at = |q: usize| {
            let last = latencies.len().saturating_sub(1);
            latencies
                .get((latencies.len() * q / 100).min(last))
                .copied()
                .unwrap_or_default()
        };
        // p95: a rarer slower read starves a voice briefly (it waits and
        // resumes) rather than failing; heads sized for the tail cost far more.
        let head = head_frames(at(95), rate, policy);
        let report = StreamReport {
            latency_p50: at(50),
            latency_p95: at(95),
            latency_p99: at(99),
            head_frames: head,
            head_bytes: 0,
            full_bytes: full,
            pool_pages: policy.voices * PAGES_PER_VOICE,
            pool_bytes: 0,
        };
        Ok(Opened {
            assets,
            frame_bytes,
            sources: registry,
            report,
            policy: *policy,
        })
    }

    /// Read every asset's `ranges` and start `decoders` threads for `worker`.
    /// Returns the streamer and the resident bytes of those ranges.
    pub(crate) fn start(
        sources: HashMap<AssetId, Arc<dyn AssetSource>>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        compressed: Vec<bool>,
        worker: StreamWorker,
        decoders: usize,
        lazy: bool,
        head_budget: usize,
    ) -> io::Result<(Self, usize)> {
        let span = crate::audit::Span::new("sample_preload");
        let mut bytes = 0;
        let mut table = HashMap::with_capacity(assets.len());
        for ((pcm, ranges), compress) in assets.iter().zip(ranges).zip(compressed) {
            if !lazy && !ranges.is_empty() {
                let mut reader = sources[&pcm.asset_id()].open()?;
                bytes += load_ranges(pcm, &mut reader, &ranges, compress)?;
            }
            table.insert(pcm.asset_id(), (ranges, compress));
        }
        drop(span);
        let sources = Arc::new(sources);
        let stop = Arc::new(AtomicBool::new(false));
        // ponytail: fairness is within one part; port shared-bank scheduling with a multi-part probe.
        // Decoders overlap reads through one worker endpoint.
        let worker = Arc::new(Mutex::new(worker));
        let mut streamer = Self {
            sources,
            ranges: Arc::new(table),
            stop,
            threads: Vec::new(),
            reloader: None,
            head_budget,
        };
        for _ in 0..decoders.max(1) {
            let thread = std::thread::Builder::new()
                .name("sampler-stream".into())
                .spawn({
                    let (sources, stop) = (streamer.sources.clone(), streamer.stop.clone());
                    let worker = worker.clone();
                    move || decode(&worker, &sources, &stop)
                })?;
            streamer.threads.push(thread);
        }
        // Restore purged start ranges a start found missing, off the audio
        // and control threads.
        let reloader = std::thread::Builder::new()
            .name("sampler-reload".into())
            .spawn({
                let assets: Arc<[Pcm]> = assets.into();
                let (sources, ranges) = (streamer.sources.clone(), streamer.ranges.clone());
                let stop = streamer.stop.clone();
                move || {
                    // Woken by the audio side (`StreamCache::set_wake`) when a
                    // start finds an asset cold, and by Drop.
                    while !stop.load(Ordering::Relaxed) {
                        if reload(&assets, &sources, &ranges, head_budget).is_err() {
                            std::thread::park_timeout(Duration::from_millis(100));
                        } else {
                            std::thread::park();
                        }
                    }
                }
            })?;
        streamer.reloader = Some(reloader);
        Ok((streamer, bytes))
    }

    /// Purge the start ranges of streamed assets not played since `before`
    /// (runtime clock). Returns the bytes freed. Their next start fails
    /// `NotReady` and marks them for `reload`.
    pub fn purge(&self, assets: &[Pcm], before: u64) -> usize {
        self.trim(assets, 0, before)
    }

    /// Purge start ranges, least recently played first, until those of
    /// `assets` take at most `budget` bytes; only assets not played since
    /// `before` are purged. Returns the bytes freed.
    pub fn trim(&self, assets: &[Pcm], budget: usize, before: u64) -> usize {
        let bytes = |pcm: &Pcm| pcm.head_bytes();
        let mut held: usize = assets.iter().map(bytes).sum();
        let mut idle: Vec<&Pcm> = assets
            .iter()
            .filter(|pcm| pcm.head_frames() > 0 && pcm.last_played() < before)
            .collect();
        idle.sort_unstable_by_key(|pcm| pcm.last_played());
        let mut freed = 0;
        for pcm in idle {
            if held <= budget {
                break;
            }
            let old = pcm.set_ranges(Vec::new()).expect("no ranges are valid");
            let n = old.iter().map(|(_, f)| f.bytes()).sum::<usize>();
            (held, freed) = (held - n, freed + n);
        }
        freed
    }

    /// Read start ranges again for assets whose start found them purged.
    /// Returns how many were reloaded.
    /// A background thread already does this when a start finds one cold.
    pub fn reload(&self, assets: &[Pcm]) -> io::Result<usize> {
        reload(assets, &self.sources, &self.ranges, self.head_budget)
    }
}

fn reload(
    assets: &[Pcm],
    sources: &HashMap<AssetId, Arc<dyn AssetSource>>,
    ranges: &HashMap<AssetId, (Vec<Range<usize>>, bool)>,
    budget: usize,
) -> io::Result<usize> {
    let mut count = 0;
    for pcm in assets {
        if pcm.resident_frames().is_some() || !pcm.take_cold() {
            continue;
        }
        let id = pcm.asset_id();
        let (Some(source), Some((ranges, compress))) = (sources.get(&id), ranges.get(&id)) else {
            continue;
        };
        // Starts get their onset from the prioritized page workers as well.
        // Cache complete start ranges only within the fixed admission budget;
        // a large/offset-heavy source keeps streaming rather than growing RSS.
        let estimate = ranges
            .iter()
            .map(|r| r.len().saturating_mul(size_of::<Frame>()))
            .sum::<usize>();
        let held = assets.iter().map(Pcm::head_bytes).sum::<usize>();
        if held.saturating_add(estimate) <= budget {
            if let Err(error) = source
                .open_stream()
                .and_then(|mut reader| load_ranges(pcm, &mut reader, ranges, *compress))
            {
                if !matches!(
                    error.kind(),
                    io::ErrorKind::InvalidData
                        | io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::NotFound
                        | io::ErrorKind::PermissionDenied
                ) {
                    pcm.mark_cold();
                }
                return Err(error);
            }
            count += 1;
        }
    }
    Ok(count)
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..).chain(self.reloader.take()) {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn decode(
    worker: &Mutex<StreamWorker>,
    sources: &HashMap<AssetId, Arc<dyn AssetSource>>,
    stop: &AtomicBool,
) {
    let worker = || worker.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut decoded = decoded::Decoded::default();
    let mut held = None;
    while !stop.load(Ordering::Relaxed) {
        let voice_job = worker().next_voice_job();
        if let Some(mut job) = voice_job {
            let asset = job.asset();
            let result = sources.get(&asset).ok_or(DecodeFailure::Unavailable)
                .and_then(|source| job.fill(|at| decoded.frame(asset, source, at)));
            worker().complete_voice(job, result);
            continue;
        }
        let Some(mut job) = held.take().or_else(|| worker().next_job()) else {
            // Failed rings retry on their deadline; idle workers remain parked as in v1.
            let retry = worker().voice_retry_delay();
            match retry { Some(delay) => std::thread::park_timeout(delay), None => std::thread::park() }
            continue;
        };
        let asset = job.key().asset;
        let result = sources.get(&asset).ok_or(DecodeFailure::Unavailable)
            .and_then(|source| decoded.read(asset, source, job.range().start, job.frames_mut()));
        if let Err(rejected) = worker().complete(job, result) {
            if rejected.reason != sampler_core::StreamError::Capacity { return; }
            held = Some(rejected.job);
            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

/// A streamed load with merged resident onsets and one source-time ring per voice.
/// The runtime services live voice demand each block; workers fill urgent voices
/// before speculative top-ups, up to v1's 8192-frame lead.
pub struct Streamed {
    pub loaded: crate::Loaded,
    pub assets: Vec<Pcm>,
    pub cache: StreamCache,
    pub streamer: Streamer,
    pub report: StreamReport,
}

impl Streamed {
    /// Read the start ranges of `opened` assets that `loaded`'s zones use and
    /// start streaming. `kept` are the assets of `loaded`'s plan, in its order.
    pub(crate) fn new(
        loaded: crate::Loaded,
        opened: Opened,
        kept: Vec<Pcm>,
    ) -> Result<Self, LoadError> {
        let Opened {
            assets,
            frame_bytes,
            sources,
            mut report,
            policy,
        } = opened;
        let invalid = |reason: String| LoadError::Invalid {
            path: "stream cache".into(),
            reason,
        };
        let (mut cache, worker) = StreamCache::voice_rings(policy.voices, policy.decoders)
            .map_err(|e| invalid(e.to_string()))?;
        let head_budget = policy.head_budget.saturating_sub(cache.bytes());
        report.pool_bytes = cache.bytes();
        report.pool_pages = report.pool_bytes / (PAGE_FRAMES * size_of::<Frame>());
        let widths: HashMap<_, _> = assets
            .iter()
            .zip(frame_bytes)
            .map(|(pcm, width)| (pcm.asset_id(), width))
            .collect();
        let widths: Vec<_> = kept.iter().map(|pcm| widths[&pcm.asset_id()]).collect();
        let (mut ranges, preload) = preload::plan(
            &loaded.instrument,
            &kept,
            &loaded.plan.preload_start_offsets(),
            &widths,
            head_budget,
        );
        if let Some(room) = policy.resident_budget {
            keep_whole(&kept, &mut ranges, &widths, room);
        }
        report.head_frames = preload;
        let compressed = preload::compression(&loaded.instrument, &kept, &ranges, preload);
        let planned_spans = ranges.iter().map(Vec::len).sum::<usize>();
        let planned_frames = ranges.iter().flatten().map(Range::len).sum::<usize>();
        let planned_bytes = ranges
            .iter()
            .zip(&widths)
            .map(|(rs, width)| rs.iter().map(|r| r.len() * width).sum::<usize>())
            .sum::<usize>();
        let head_assets = ranges.iter().filter(|rs| !rs.is_empty()).count();
        let (streamer, bytes) = Streamer::start(
            sources,
            &kept,
            ranges,
            compressed,
            worker,
            policy.decoders,
            policy.lazy,
            head_budget,
        )
        .map_err(|e| invalid(e.to_string()))?;
        report.head_bytes = bytes;
        if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
            let (packed_spans, packed_frames) = kept
                .iter()
                .map(Pcm::head_compression)
                .fold((0, 0), |(s, f), (a, b)| (s + a, f + b));
            let widths = [
                widths.iter().filter(|&&w| w == 4).count(),
                widths.iter().filter(|&&w| w == 6).count(),
                widths.iter().filter(|&&w| w == 8).count(),
            ];
            eprintln!(
                "AUDIT {{\"stage\":\"stream_head_owners\",\"assets\":{},\"head_assets\":{head_assets},\"planned_spans\":{planned_spans},\"planned_frames\":{planned_frames},\"planned_native_bytes\":{planned_bytes},\"packed_spans\":{packed_spans},\"packed_frames\":{packed_frames},\"actual_head_bytes\":{bytes},\"ring_virtual_bytes\":{},\"preload\":{preload},\"source_width_counts\":{widths:?}}}",
                kept.len(),
                report.pool_bytes
            );
        }
        cache.set_wake(
            streamer
                .threads
                .iter()
                .chain(&streamer.reloader)
                .map(|t| t.thread().clone())
                .collect(),
        );
        drop(assets);
        Ok(Self {
            loaded,
            assets: kept,
            cache,
            streamer,
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_resident_head_and_voice_ring_worker_render_the_complete_source_offline() {
        struct Ramp;
        impl AssetSource for Ramp {
            fn open(&self) -> io::Result<SampleReader> {
                Ok(SampleReader::custom(48000,20000,|start,out| {
                    for (i,f) in out.iter_mut().enumerate() { *f=[(start+i) as f32/32768.;2]; }
                    Ok(())
                }))
            }
        }
        let instrument=ir::Instrument {
            assets:vec![ir::Asset {location:ir::AssetLocation::Path("ramp".into()),
                encoding:ir::Encoding::Unknown,root_key:None,loops:vec![]}],
            zones:vec![ir::Zone::new(ir::AssetRef(0))],..Default::default()
        };
        let streamed=crate::stream_instrument(instrument,vec![Arc::new(Ramp)],vec!["ramp".into()],
            &crate::Options::default(),&StreamPolicy {voices:1,decoders:1,..Default::default()}).unwrap();
        assert_eq!(streamed.report.head_frames,preload::PRELOAD);
        assert_eq!(streamed.assets[0].head_frames(),preload::PRELOAD);
        assert_eq!(streamed.cache.bytes(),8192*size_of::<Frame>());
        let limits=sampler_core::Limits::for_plan(&streamed.loaded.plan,1,1);
        let mut rt=sampler_core::Runtime::new(streamed.loaded.plan,limits).unwrap().with_stream_cache(streamed.cache);
        rt.set_offline(true);
        rt.trigger(sampler_core::Input {protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None},60,1.).unwrap();
        let mut output=vec![[0.;2];12000];
        for block in output.chunks_mut(32) {rt.render(block).unwrap();}
        assert_eq!(rt.take_stream_fault(),None);
        assert_eq!(rt.stream_underruns(),0);
        for (i,f) in output.into_iter().enumerate() {assert_eq!(f,[i as f32/32768.;2],"frame {i}");}
        drop(streamed.streamer);
    }

    #[test]
    fn lazy_reload_reserves_the_voice_ring_bytes_in_the_total_budget() {
        struct Head(Arc<std::sync::atomic::AtomicUsize>);
        impl AssetSource for Head {
            fn header(&self) -> Option<(u32, usize)> { Some((48000, 8192)) }
            fn frame_bytes(&self) -> Option<usize> { Some(4) }
            fn open(&self) -> io::Result<SampleReader> {
                let reads = self.0.clone();
                Ok(SampleReader::custom(48000, 8192, move |_, out| {
                    reads.fetch_add(1, Ordering::Relaxed);
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let instrument = ir::Instrument {
            assets: vec![ir::Asset {
                location: ir::AssetLocation::Path("budget".into()),
                encoding: ir::Encoding::Unknown, root_key: None, loops: vec![],
            }],
            zones: vec![ir::Zone::new(ir::AssetRef(0))],
            ..Default::default()
        };
        let streamed = crate::stream_instrument(
            instrument, vec![Arc::new(Head(reads.clone()))], vec!["budget".into()],
            &crate::Options::default(),
            &StreamPolicy {
                voices: 1, decoders: 1, lazy: true,
                head_budget: 8192 * size_of::<Frame>() + 1,
                ..Default::default()
            },
        ).unwrap();
        assert_eq!(streamed.report.head_bytes, 0);
        streamed.assets[0].mark_cold();
        streamed.streamer.reload(&streamed.assets).unwrap();
        // Join the background reloader too, so either consumer must honor the cap.
        drop(streamed.streamer);
        assert_eq!(reads.load(Ordering::Relaxed), 0);
        assert_eq!(streamed.assets[0].head_bytes(), 0);
    }

    #[test]
    fn headers_open_in_bounded_parallel_workers_and_keep_asset_order() {
        struct Header {
            frames: usize,
            active: Arc<std::sync::atomic::AtomicUsize>,
            peak: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl AssetSource for Header {
            fn open(&self) -> io::Result<SampleReader> {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                self.active.fetch_sub(1, Ordering::SeqCst);
                Ok(SampleReader::custom(48000, self.frames, |_, out| {
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sources = (1..=8)
            .map(|frames| {
                (
                    Arc::new(Header {
                        frames,
                        active: active.clone(),
                        peak: peak.clone(),
                    }) as Arc<dyn AssetSource>,
                    std::path::Path::new("test"),
                )
            })
            .collect();
        let opened =
            Streamer::open(sources, 48000, &StreamPolicy::default(), 0, &|| false).unwrap();
        assert_eq!(
            opened
                .assets
                .iter()
                .map(Pcm::frame_count)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<_>>()
        );
        assert!((1..=8).contains(&peak.load(Ordering::SeqCst)));
    }

    #[test]
    fn lazy_heads_are_not_read_until_requested_and_respect_the_budget() {
        struct Head(Arc<std::sync::atomic::AtomicUsize>);
        impl AssetSource for Head {
            fn open(&self) -> io::Result<SampleReader> {
                let reads = self.0.clone();
                Ok(SampleReader::custom(48000, 8192, move |_, out| {
                    reads.fetch_add(1, Ordering::Relaxed);
                    out.fill([0.5; 2]);
                    Ok(())
                }))
            }
        }
        let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pcm = Pcm::streamed(48000, 8192).unwrap();
        let sources = HashMap::from([(
            pcm.asset_id(),
            Arc::new(Head(reads.clone())) as Arc<dyn AssetSource>,
        )]);
        let (_, worker) = StreamCache::new(1).unwrap();
        let (streamer, bytes) = Streamer::start(
            sources,
            std::slice::from_ref(&pcm),
            vec![vec![0..4096]],
            vec![true],
            worker,
            1,
            true,
            1,
        )
        .unwrap();
        assert_eq!(
            (bytes, reads.load(Ordering::Relaxed), pcm.head_bytes()),
            (0, 0, 0)
        );
        // No on-demand reload can exceed a budget smaller than this head.
        assert_eq!(streamer.reload(std::slice::from_ref(&pcm)).unwrap(), 0);
        assert_eq!(pcm.head_bytes(), 0);
    }

    #[test]
    fn v1_ram_mode_fills_small_samples_first_and_streams_over_budget() {
        let assets: Vec<_> = [1000, 4000, 2000].into_iter().map(|n| Pcm::streamed(48000, n).unwrap()).collect();
        let mut heads = vec![vec![0..100]; 3];
        let needed = keep_whole(&assets, &mut heads, &[8;3], 24000 + 800);
        assert_eq!(heads[0], vec![0..1000]);
        assert_eq!(heads[2], vec![0..2000]);
        assert_eq!(heads[1], vec![0..100], "large sample falls back to streaming");
        assert_eq!(needed, (4000 - 100) * 8);
        keep_whole(&assets, &mut heads, &[8;3], usize::MAX);
        assert_eq!(heads[1], vec![0..4000]);
    }


    #[test]
    fn trimming_purges_idle_heads_until_within_budget() {
        let head = [[0.5f32; 2]; 1000];
        let assets: Vec<Pcm> = (0..3)
            .map(|_| Pcm::headed(48000, 8000, &head).unwrap())
            .collect();
        let (_, worker) = StreamCache::new(1).unwrap();
        let (streamer, _) = Streamer::start(
            HashMap::new(),
            &assets,
            vec![vec![]; 3],
            vec![true; 3],
            worker,
            1,
            false,
            usize::MAX,
        )
        .unwrap();
        // Packed: 16-bit mono.
        let size = assets[0].head_bytes();
        assert_eq!(size, 1000 * 2);
        // Nothing has played since 0: nothing is idle before it.
        assert_eq!(streamer.trim(&assets, 0, 0), 0);
        assert_eq!(streamer.trim(&assets, size * 3 / 2, 1), 2 * size);
        let left: usize = assets.iter().map(Pcm::head_frames).sum();
        assert_eq!(left, 1000);
        assert_eq!(streamer.purge(&assets, 1), size);
    }

    #[test]
    fn random_access_reads_match_a_full_decode() {
        let dir = std::env::temp_dir().join(format!("kontakt-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcm: Vec<i32> = (0..40000 * 2).map(|i| (i * 37 % 2000) - 1000).collect();
        let spec = ncw::PcmSpec {
            channels: 2,
            bits_per_sample: 16,
            sample_rate: 44100,
        };
        let ncw = ncw::encode_pcm(&pcm, spec, ncw::StereoMode::Direct).unwrap();
        let mut wav =
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data"
                .to_vec();
        let samples: Vec<u8> = (0..40000i32)
            .flat_map(|i| ((i * 11) as i16).to_le_bytes())
            .collect();
        wav.extend((samples.len() as u32).to_le_bytes());
        wav.extend(samples);
        for (name, bytes) in [("a.ncw", ncw), ("b.wav", wav)] {
            let path = dir.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let full = crate::decode(&bytes).unwrap().frames;
            let source = crate::Samples::new(&dir).source(&path).unwrap();
            let before = DISK_READ.load(Ordering::Relaxed);
            let mut reader = SampleReader::open(&source).unwrap();
            assert_eq!((reader.frames(), reader.rate()), (40000, 44100));
            assert_eq!(reader.frame_bytes(), 4);
            for range in [0..40000, 511..1025, 39999..40000, 1000..1000, 32000..33000] {
                let mut out = vec![[0.0; 2]; range.len()];
                reader.read(range.start, &mut out).unwrap();
                assert_eq!(out, full[range], "{name}");
            }
            assert!(reader.read(39999, &mut [[0.0; 2]; 2]).is_err());
            assert_eq!(DISK_READ.load(Ordering::Relaxed), before, "loading reads do not count");
            let mut reader = source.open_stream().unwrap();
            reader.read(0, &mut vec![[0.; 2]; 40000]).unwrap();
            assert!(DISK_READ.load(Ordering::Relaxed) >= before + bytes.len() as u64, "physical playback reads count");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
