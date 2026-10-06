//! Streamed instruments: random-access sample reads, resident heads sized from
//! measured read latency, and the decode worker thread behind the core's
//! page cache. Only heads and a bounded page pool are resident.

use crate::{LoadError, samples::Source};
use sampler_core::{AssetId, DecodeFailure, PAGE_FRAMES, Pcm, StreamCache, StreamWorker};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

type Frame = [f32; 2];

/// A sample's bytes as a seekable stream, decrypted on the fly.
struct Bytes {
    file: File,
    source: Source,
    position: u64,
}

impl Read for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.source.size.saturating_sub(self.position);
        let len = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        let read = self.file.read(&mut buf[..len])?;
        if let Some(key) = &self.source.key {
            key.apply_at(self.position, &mut buf[..read]);
        }
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Bytes {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let position = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.source.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.file
            .seek(SeekFrom::Start(self.source.offset + position))?;
        self.position = position;
        Ok(position)
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
        let mut bytes = Bytes {
            file: File::open(&source.path)?,
            source: source.clone(),
            position: 0,
        };
        bytes.seek(SeekFrom::Start(0))?;
        let mut head = Vec::new();
        (&mut bytes).take(1 << 16).read_to_end(&mut head)?;
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
    pub latency_p99: Duration,
    pub head_pages: usize,
    pub head_bytes: usize,
    pub pool_pages: usize,
    pub pool_bytes: usize,
}

/// How much to keep resident.
#[derive(Clone, Copy, Debug)]
pub struct StreamPolicy {
    /// Fastest a voice consumes source frames (step), for head sizing.
    pub max_step: f64,
    /// Worst wait between a page request and its decode starting (worker poll
    /// and the host's service period), on top of measured read latency.
    pub slack: Duration,
    /// Voices that can stream at once; each needs a few pool pages.
    pub voices: usize,
}

impl Default for StreamPolicy {
    fn default() -> Self {
        Self {
            max_step: 2.0,
            slack: Duration::from_millis(10),
            voices: 256,
        }
    }
}

/// Pool pages per streaming voice: the page it reads, the next, and one more
/// for a window or crossfade spanning a page boundary.
const PAGES_PER_VOICE: usize = 3;

/// Frames of head an asset needs so a voice at `policy.max_step` never
/// reaches page one before it can be read: whole pages.
pub(crate) fn head_pages(latency: Duration, rate: u32, policy: &StreamPolicy) -> usize {
    let wait = (latency + policy.slack).as_secs_f64();
    let frames = wait * f64::from(rate) * policy.max_step;
    // One extra page: page one is requested only once the horizon reaches it.
    (frames / PAGE_FRAMES as f64).ceil() as usize + 1
}

/// Loaded heads: the assets, where the rest of each lives, and head length.
pub(crate) struct Heads {
    pub assets: Vec<Pcm>,
    pub sources: HashMap<AssetId, Source>,
    pub report: StreamReport,
    pub frames: usize,
}

/// Control side: open sources, read heads, own the decode thread. Purge and
/// reload heads here; the runtime owns the page cache.
pub struct Streamer {
    sources: Arc<HashMap<AssetId, Source>>,
    head_frames: usize,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Streamer {
    /// Decode heads of `pages` pages for every source. `measure` reads one
    /// page from up to that many sources first, to size heads by `policy`.
    pub(crate) fn heads(
        sources: Vec<(Source, &std::path::Path)>,
        rate: u32,
        policy: &StreamPolicy,
        probe: usize,
    ) -> Result<Heads, LoadError> {
        let error = |path: &std::path::Path, e: io::Error| LoadError::io(path, e);
        let mut latencies = Vec::new();
        let mut page = vec![[0.0; 2]; PAGE_FRAMES];
        for (source, path) in sources.iter().take(probe) {
            let begin = Instant::now();
            let mut reader = SampleReader::open(source).map_err(|e| error(path, e))?;
            let n = reader.frames().min(PAGE_FRAMES);
            reader.read(0, &mut page[..n]).map_err(|e| error(path, e))?;
            latencies.push(begin.elapsed());
        }
        latencies.sort_unstable();
        let at = |q: usize| latencies.get(latencies.len() * q / 100).copied();
        let p99 = at(99).or(latencies.last().copied()).unwrap_or_default();
        let pages = head_pages(p99, rate, policy);
        let mut assets = Vec::with_capacity(sources.len());
        let mut registry = HashMap::with_capacity(sources.len());
        for (source, path) in sources {
            let mut reader = SampleReader::open(&source).map_err(|e| error(path, e))?;
            let frames = reader.frames();
            let mut head = vec![[0.0; 2]; frames.min(pages * PAGE_FRAMES)];
            reader.read(0, &mut head).map_err(|e| error(path, e))?;
            let invalid = |e: sampler_core::Error| LoadError::Invalid {
                path: path.into(),
                reason: e.to_string(),
            };
            let pcm = if head.len() == frames {
                Pcm::new(reader.rate(), head.into()).map_err(invalid)?
            } else {
                Pcm::headed(reader.rate(), frames, head.into()).map_err(invalid)?
            };
            registry.insert(pcm.asset_id(), source);
            assets.push(pcm);
        }
        let report = StreamReport {
            latency_p50: at(50).unwrap_or_default(),
            latency_p99: p99,
            head_pages: pages,
            head_bytes: assets.iter().map(Pcm::resident_bytes).sum(),
            pool_pages: policy.voices * PAGES_PER_VOICE,
            pool_bytes: policy.voices * PAGES_PER_VOICE * PAGE_FRAMES * size_of::<Frame>(),
        };
        Ok(Heads {
            assets,
            sources: registry,
            report,
            frames: pages * PAGE_FRAMES,
        })
    }

    /// Start the decode thread for `cache`'s worker.
    pub(crate) fn start(
        sources: HashMap<AssetId, Source>,
        head_frames: usize,
        worker: StreamWorker,
    ) -> Self {
        let sources = Arc::new(sources);
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("sampler-stream".into())
            .spawn({
                let (sources, stop) = (sources.clone(), stop.clone());
                move || decode(worker, &sources, &stop)
            })
            .expect("spawn the stream decode thread");
        Self {
            sources,
            head_frames,
            stop,
            thread: Some(thread),
        }
    }

    /// Purge the heads of streamed assets not played since `before` (runtime
    /// clock). Returns the bytes freed. Their next start fails `NotReady`
    /// and marks them for `reload`.
    pub fn purge(&self, assets: &[Pcm], before: u64) -> usize {
        let mut freed = 0;
        for pcm in assets {
            if pcm.resident_frames().is_none() && pcm.last_played() < before {
                freed += pcm
                    .set_head(Box::default())
                    .expect("an empty head is valid")
                    .len()
                    * size_of::<Frame>();
            }
        }
        freed
    }

    /// Read heads again for assets whose start found theirs purged. Returns
    /// how many were reloaded.
    pub fn reload(&self, assets: &[Pcm]) -> io::Result<usize> {
        let mut count = 0;
        for pcm in assets {
            if pcm.resident_frames().is_some() || !pcm.take_cold() {
                continue;
            }
            let Some(source) = self.sources.get(&pcm.asset_id()) else {
                continue;
            };
            let mut reader = SampleReader::open(source)?;
            let mut head = vec![[0.0; 2]; reader.frames().min(self.head_frames)];
            reader.read(0, &mut head)?;
            pcm.set_head(head.into())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            count += 1;
        }
        Ok(count)
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Open readers kept between pages of the same assets.
const OPEN_READERS: usize = 64;

fn decode(mut worker: StreamWorker, sources: &HashMap<AssetId, Source>, stop: &AtomicBool) {
    // ponytail: linear LRU over a few dozen readers; a map if that shows up.
    let mut readers: Vec<(AssetId, SampleReader, u64)> = Vec::new();
    let mut tick = 0u64;
    let mut held = None;
    while !stop.load(Ordering::Relaxed) {
        let Some(mut job) = held.take().or_else(|| worker.next_job()) else {
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
        if let Err(rejected) = worker.complete(job, result) {
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
/// `service_streaming` every block with a horizon of at least one page.
pub struct Streamed {
    pub loaded: crate::Loaded,
    pub assets: Vec<Pcm>,
    pub cache: StreamCache,
    pub streamer: Streamer,
    pub report: StreamReport,
}

impl Streamed {
    pub(crate) fn new(loaded: crate::Loaded, heads: Heads) -> Result<Self, LoadError> {
        let Heads {
            assets,
            sources,
            mut report,
            frames: head_frames,
        } = heads;
        let (cache, worker) =
            StreamCache::new(report.pool_pages.max(1)).map_err(|e| LoadError::Invalid {
                path: "stream cache".into(),
                reason: e.to_string(),
            })?;
        report.pool_bytes = cache.bytes();
        Ok(Self {
            loaded,
            assets,
            cache,
            streamer: Streamer::start(sources, head_frames, worker),
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_access_reads_match_a_full_decode() {
        let dir = std::env::temp_dir().join(format!("kontakt-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcm: Vec<i32> = (0..3000 * 2).map(|i| (i * 37 % 2000) - 1000).collect();
        let spec = ncw::PcmSpec {
            channels: 2,
            bits_per_sample: 16,
            sample_rate: 44100,
        };
        let ncw = ncw::encode_pcm(&pcm, spec, ncw::StereoMode::Direct).unwrap();
        let mut wav =
            b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data"
                .to_vec();
        let samples: Vec<u8> = (0..3000i16).flat_map(|i| (i * 11).to_le_bytes()).collect();
        wav.extend((samples.len() as u32).to_le_bytes());
        wav.extend(samples);
        for (name, bytes) in [("a.ncw", ncw), ("b.wav", wav)] {
            let path = dir.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let full = crate::decode(&bytes).unwrap().frames;
            let source = crate::Samples::new(&dir).source(&path).unwrap();
            let mut reader = SampleReader::open(&source).unwrap();
            assert_eq!((reader.frames(), reader.rate()), (3000, 44100));
            for range in [0..3000, 511..1025, 2999..3000, 1000..1000] {
                let mut out = vec![[0.0; 2]; range.len()];
                reader.read(range.start, &mut out).unwrap();
                assert_eq!(out, full[range], "{name}");
            }
            assert!(reader.read(2999, &mut [[0.0; 2]; 2]).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
