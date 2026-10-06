//! Streamed instruments: random-access sample reads, resident heads sized from
//! measured read latency, and the decode worker thread behind the core's
//! page cache. Only heads and a bounded page pool are resident.

use crate::{LoadError, samples::Source};
use sampler_core::{AssetId, DecodeFailure, PAGE_FRAMES, Pcm, StreamCache, StreamWorker};
use sampler_ir as ir;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

type Frame = [f32; 2];

/// A sample's bytes as a seekable stream, decrypted on the fly. Reads go
/// through one buffer: decoders read a few bytes at a time.
struct Bytes {
    file: File,
    source: Source,
    position: u64,
    /// Decrypted bytes from member offset `at`.
    buffer: Vec<u8>,
    at: u64,
}

/// Bytes per file read.
const BUFFER: usize = 64 << 10;

impl Bytes {
    fn open(source: &Source) -> io::Result<Self> {
        Ok(Self {
            file: File::open(&source.path)?,
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
            self.file
                .seek(SeekFrom::Start(self.source.offset + self.position))?;
            self.buffer.resize(len, 0);
            let mut filled = 0;
            while filled < len {
                match self.file.read(&mut self.buffer[filled..])? {
                    0 => break,
                    n => filled += n,
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

enum Codec {
    Wav(crate::samples::Wav),
    Ncw(Box<ncw::NcwReader<Bytes>>),
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
    pub fn open(source: &Source) -> io::Result<Self> {
        let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        let mut bytes = Bytes::open(source)?;
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
        let reader = ncw::NcwReader::read(bytes).map_err(|e| invalid(format!("NCW: {e}")))?;
        Ok(Self {
            rate: reader.header.sample_rate,
            frames: reader.header.num_samples as usize,
            codec: Codec::Ncw(Box::new(reader)),
            bytes: None,
            scratch: Vec::new(),
        })
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Fill `out` with frames from `start`.
    pub fn read(&mut self, start: usize, out: &mut [Frame]) -> io::Result<()> {
        let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
        if start + out.len() > self.frames {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        match &mut self.codec {
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
    /// Output frames each start keeps resident (times its zone's step in
    /// source frames): the measured latency budget.
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
    /// Voices that can stream at once; each needs a few pool pages.
    pub voices: usize,
    /// Decode threads.
    pub decoders: usize,
}

impl Default for StreamPolicy {
    fn default() -> Self {
        Self {
            headroom: 2.0,
            max_step: 4.0,
            slack: Duration::from_millis(10),
            voices: 256,
            decoders: 4,
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
    ((latency + policy.slack).as_secs_f64() * f64::from(rate)).ceil() as usize
}

/// Kernel guard frames around a start, beyond the widest resampling window.
const GUARD: u64 = 128;

/// Fastest a zone reads its asset, in source frames per output frame: its
/// highest key plus `policy.headroom`, its and its group's tuning, and the
/// asset's rate against the engine's.
fn zone_step(zone: &ir::Zone, groups: &[ir::Group], ratio: f64, policy: &StreamPolicy) -> f64 {
    let keys = match zone.pitch {
        ir::KeyTracking::Tracked { root } => f64::from(zone.keys.high) - f64::from(root),
        ir::KeyTracking::Scaled {
            root,
            cents_per_key,
        } => {
            let span = f64::from(zone.keys.high) - f64::from(root);
            let low = f64::from(zone.keys.low) - f64::from(root);
            (span * f64::from(cents_per_key)).max(low * f64::from(cents_per_key)) / 100.0
        }
        ir::KeyTracking::Fixed => 0.0,
    };
    let group = zone.group.map_or(0.0, |g| groups[g.0].tune.semitones());
    let semitones = keys + zone.tune.semitones() + group + policy.headroom;
    (ratio * (semitones / 12.0).exp2()).min(policy.max_step)
}

/// Frame ranges of each asset that zone starts read before streaming catches
/// up (`head` output frames at the zone's step, plus a guard): forward from
/// the start, or back from the end when reversed. Ascending and merged.
pub(crate) fn start_ranges(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    rate: u32,
    head: usize,
    policy: &StreamPolicy,
) -> Vec<Vec<Range<usize>>> {
    let mut ranges = vec![Vec::new(); assets.len()];
    for zone in &instrument.zones {
        let (asset, playback) = (zone.asset.0, &zone.playback);
        let pcm = &assets[asset];
        let ratio = f64::from(pcm.sample_rate()) / f64::from(rate);
        let step = zone_step(zone, &instrument.groups, ratio, policy);
        let frames = (head as f64 * step).ceil() as u64 + 2 * GUARD;
        let length = pcm.frame_count() as u64;
        let end = playback.end.unwrap_or(length).min(length);
        let (from, to) = if playback.reverse {
            (end.saturating_sub(frames), end)
        } else {
            let from = playback.start.min(length).saturating_sub(GUARD);
            (from, (from + frames).min(length))
        };
        if from < to {
            ranges[asset].push(from as usize..to as usize);
        }
    }
    for list in &mut ranges {
        list.sort_unstable_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(list.len());
        for range in list.drain(..) {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        *list = merged;
    }
    ranges
}

/// Streamed assets before their start ranges are read.
pub(crate) struct Opened {
    pub assets: Vec<Pcm>,
    pub sources: HashMap<AssetId, Source>,
    pub report: StreamReport,
    pub head: usize,
    pub rate: u32,
    pub policy: StreamPolicy,
}

/// Read `ranges` of `pcm` from `reader` and make them resident; their bytes.
fn load_ranges(pcm: &Pcm, reader: &mut SampleReader, ranges: &[Range<usize>]) -> io::Result<usize> {
    let mut resident = Vec::with_capacity(ranges.len());
    for range in ranges {
        let mut frames = vec![[0.0; 2]; range.len()];
        reader.read(range.start, &mut frames)?;
        resident.push((range.start, frames.into_boxed_slice()));
    }
    let bytes = resident.iter().map(|(_, f)| f.len()).sum::<usize>() * size_of::<Frame>();
    pcm.set_ranges(resident)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(bytes)
}

/// Control side: where every streamed asset lives, its start ranges, and the
/// decode thread. Purge and reload start ranges here; the runtime owns the
/// page cache.
pub struct Streamer {
    sources: Arc<HashMap<AssetId, Source>>,
    ranges: HashMap<AssetId, Vec<Range<usize>>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Streamer {
    /// Open every source as a streamed asset, timing an open plus first-page
    /// read on up to `probe` of them to size heads by `policy`.
    pub(crate) fn open(
        sources: Vec<(Source, &std::path::Path)>,
        rate: u32,
        policy: &StreamPolicy,
        probe: usize,
    ) -> Result<Opened, LoadError> {
        let mut latencies = Vec::new();
        let mut page = vec![[0.0; 2]; PAGE_FRAMES];
        let mut assets = Vec::with_capacity(sources.len());
        let mut registry = HashMap::with_capacity(sources.len());
        let mut full = 0;
        for (i, (source, path)) in sources.into_iter().enumerate() {
            let begin = Instant::now();
            let mut reader = SampleReader::open(&source).map_err(|e| LoadError::io(path, e))?;
            if i < probe {
                let n = reader.frames().min(PAGE_FRAMES);
                reader
                    .read(0, &mut page[..n])
                    .map_err(|e| LoadError::io(path, e))?;
                latencies.push(begin.elapsed());
            }
            full += reader.frames() as u64 * size_of::<Frame>() as u64;
            let pcm =
                Pcm::streamed(reader.rate(), reader.frames()).map_err(|e| LoadError::Invalid {
                    path: path.into(),
                    reason: e.to_string(),
                })?;
            registry.insert(pcm.asset_id(), source);
            assets.push(pcm);
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
            sources: registry,
            report,
            head,
            rate,
            policy: *policy,
        })
    }

    /// Read every asset's `ranges` and start `decoders` threads for `worker`.
    /// Returns the streamer and the resident bytes of those ranges.
    pub(crate) fn start(
        sources: HashMap<AssetId, Source>,
        assets: &[Pcm],
        ranges: Vec<Vec<Range<usize>>>,
        worker: StreamWorker,
        decoders: usize,
    ) -> io::Result<(Self, usize)> {
        let mut bytes = 0;
        let mut table = HashMap::with_capacity(assets.len());
        for (pcm, ranges) in assets.iter().zip(ranges) {
            if !ranges.is_empty() {
                let mut reader = SampleReader::open(&sources[&pcm.asset_id()])?;
                bytes += load_ranges(pcm, &mut reader, &ranges)?;
            }
            table.insert(pcm.asset_id(), ranges);
        }
        let sources = Arc::new(sources);
        let stop = Arc::new(AtomicBool::new(false));
        // Several decoders overlap reads, so one slow read does not hold up
        // the pages queued behind it; they share the single worker endpoint.
        let worker = Arc::new(Mutex::new(worker));
        let mut streamer = Self {
            sources,
            ranges: table,
            stop,
            threads: Vec::new(),
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
        let bytes = |pcm: &Pcm| pcm.head_frames() * size_of::<Frame>();
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
            let n = old.iter().map(|(_, f)| f.len()).sum::<usize>() * size_of::<Frame>();
            (held, freed) = (held - n, freed + n);
        }
        freed
    }

    /// Read start ranges again for assets whose start found them purged.
    /// Returns how many were reloaded.
    pub fn reload(&self, assets: &[Pcm]) -> io::Result<usize> {
        let mut count = 0;
        for pcm in assets {
            if pcm.resident_frames().is_some() || !pcm.take_cold() {
                continue;
            }
            let id = pcm.asset_id();
            let (Some(source), Some(ranges)) = (self.sources.get(&id), self.ranges.get(&id)) else {
                continue;
            };
            load_ranges(pcm, &mut SampleReader::open(source)?, ranges)?;
            count += 1;
        }
        Ok(count)
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Open readers kept per decode thread between pages of the same assets
/// (each holds a 64 KiB read buffer).
const OPEN_READERS: usize = 32;

fn decode(worker: &Mutex<StreamWorker>, sources: &HashMap<AssetId, Source>, stop: &AtomicBool) {
    let worker = || {
        worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    // ponytail: linear LRU over a few dozen readers; a map if that shows up.
    let mut readers: Vec<(AssetId, SampleReader, u64)> = Vec::new();
    let mut tick = 0u64;
    let mut held = None;
    while !stop.load(Ordering::Relaxed) {
        let Some(mut job) = held.take().or_else(|| worker().next_job()) else {
            // ponytail: polling; a wake from the audio side if 0.5 ms matters.
            std::thread::sleep(Duration::from_micros(500));
            continue;
        };
        tick += 1;
        let asset = job.key().asset;
        let index = match readers.iter().position(|(id, ..)| *id == asset) {
            Some(i) => Some(i),
            None => sources
                .get(&asset)
                .and_then(|s| SampleReader::open(s).ok())
                .map(|r| {
                    if readers.len() == OPEN_READERS {
                        let oldest = (0..readers.len()).min_by_key(|&i| readers[i].2).unwrap();
                        readers.swap_remove(oldest);
                    }
                    readers.push((asset, r, tick));
                    readers.len() - 1
                }),
        };
        let result = match index {
            Some(i) => {
                readers[i].2 = tick;
                let start = job.range().start;
                readers[i]
                    .1
                    .read(start, job.frames_mut())
                    .map_err(|_| DecodeFailure::InvalidSamples)
            }
            None => Err(DecodeFailure::Unavailable),
        };
        let completed = worker().complete(job, result);
        if let Err(rejected) = completed {
            if rejected.reason != sampler_core::StreamError::Capacity {
                return;
            }
            // The audio side has not drained completions yet: retry later.
            held = Some(rejected.job);
            std::thread::sleep(Duration::from_micros(500));
        }
    }
}

/// A streamed load: the plan's assets hold only heads; `cache` goes to the
/// runtime (`Runtime::with_stream_cache`), which must call
/// `service_streaming` every block with a horizon of at least
/// `report.head_frames` plus the block length: a page is then requested no
/// later than the latency budget before a voice reads it.
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
            sources,
            mut report,
            head,
            rate,
            policy,
        } = opened;
        let invalid = |reason: String| LoadError::Invalid {
            path: "stream cache".into(),
            reason,
        };
        let (cache, worker) =
            StreamCache::new(report.pool_pages.max(1)).map_err(|e| invalid(e.to_string()))?;
        report.pool_bytes = cache.bytes();
        let ranges = start_ranges(&loaded.instrument, &kept, rate, head, &policy);
        let (streamer, bytes) = Streamer::start(sources, &kept, ranges, worker, policy.decoders)
            .map_err(|e| invalid(e.to_string()))?;
        report.head_bytes = bytes;
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
    fn trimming_purges_idle_heads_until_within_budget() {
        let head = [[0.5f32; 2]; 1000];
        let assets: Vec<Pcm> = (0..3)
            .map(|_| Pcm::headed(48000, 8000, &head).unwrap())
            .collect();
        let (_, worker) = StreamCache::new(1).unwrap();
        let (streamer, _) =
            Streamer::start(HashMap::new(), &assets, vec![vec![]; 3], worker, 1).unwrap();
        let size = 1000 * size_of::<Frame>();
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
            let mut reader = SampleReader::open(&source).unwrap();
            assert_eq!((reader.frames(), reader.rate()), (40000, 44100));
            for range in [0..40000, 511..1025, 39999..40000, 1000..1000, 32000..33000] {
                let mut out = vec![[0.0; 2]; range.len()];
                reader.read(range.start, &mut out).unwrap();
                assert_eq!(out, full[range], "{name}");
            }
            assert!(reader.read(39999, &mut [[0.0; 2]; 2]).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
