//! Sample decoding with random access by frame.
//!
//! Every format goes through [`SampleReader`], so the loader decodes only the
//! resident spans of a sample and the streamer fetches the rest on demand.
//! Archive members are decrypted in place while reading: the NKX resource
//! cipher is position-relative, so any byte offset is addressable.

use anyhow::{Context, Result, bail, ensure};
use ni_file::{
    nis::LibraryKey,
    nkr::{Archive, Entry},
};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{self, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
};

/// Return freed heap memory to the system. Loading allocates and frees far
/// more than it keeps (decompressed presets, parse trees, decode buffers on
/// every core), and glibc keeps freed pages of its per-thread arenas
/// resident; call once a load finished. A no-op elsewhere.
pub fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: glibc's malloc_trim is thread-safe and has no preconditions.
        unsafe { malloc_trim(0) };
    }
}

/// One stereo frame. Mono sources are duplicated to both channels.
pub type Frame = [f32; 2];

/// Resident frames in the narrowest format that holds them exactly: 16- and
/// 24-bit sources keep their own resolution (4 or 6 bytes per stereo frame
/// instead of 8) and the voice kernel converts on the fly. Samples are
/// interleaved left/right.
pub enum Pcm {
    F32(Box<[Frame]>),
    I16(Box<[i16]>),
    /// The top 16 bits and, in their own plane, the low byte: decoding stays
    /// an elementwise pass the compiler vectorizes.
    I24(Box<[i16]>, Box<[u8]>),
}

const I16_SCALE: f32 = 32768.0;
const I24_SCALE: f32 = 8388608.0;

impl Pcm {
    /// Bytes per frame [`Pcm::pack`] uses at most for a source of `bits`
    /// resolution (32-bit and float sources stay f32).
    pub fn frame_bytes(bits: Option<u16>) -> usize {
        match bits {
            Some(..=16) => 4,
            Some(..=24) => 6,
            _ => 8,
        }
    }

    /// Store `frames` in the narrowest exact format.
    pub fn pack(frames: &[Frame]) -> Self {
        let samples = frames.as_flattened();
        // Chunked so the check vectorizes yet fails fast on wider data.
        let exact = |scale: f32| {
            samples
                .chunks(256)
                .all(|c| c.iter().fold(true, |ok, &x| ok & exact(x, scale)))
        };
        if exact(I16_SCALE) {
            return Self::I16(samples.iter().map(|&x| (x * I16_SCALE) as i16).collect());
        }
        if exact(I24_SCALE) {
            let q = |x: f32| (x * I24_SCALE) as i32;
            return Self::I24(
                samples.iter().map(|&x| (q(x) >> 8) as i16).collect(),
                samples.iter().map(|&x| q(x) as u8).collect(),
            );
        }
        Self::F32(frames.into())
    }

    pub fn len(&self) -> usize {
        match self {
            Self::F32(d) => d.len(),
            Self::I16(d) | Self::I24(d, _) => d.len() / 2,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bytes(&self) -> usize {
        match self {
            Self::F32(d) => size_of_val(&**d),
            Self::I16(d) => size_of_val(&**d),
            Self::I24(high, low) => size_of_val(&**high) + size_of_val(&**low),
        }
    }

    /// Frames `[at, at + buf.len())`: borrowed when stored as f32, otherwise
    /// decoded into `buf`. `None` if out of range.
    #[inline]
    pub fn window<'a>(&'a self, at: usize, buf: &'a mut [Frame]) -> Option<&'a [Frame]> {
        match self {
            Self::F32(d) => d.get(at..at + buf.len()),
            _ => self.decode(at, buf).then_some(buf),
        }
    }

    /// Decode frames `[at, at + out.len())` into `out`; false if out of range.
    /// Uses an AVX2 build when the CPU has it.
    #[inline]
    pub fn decode(&self, at: usize, out: &mut [Frame]) -> bool {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.decode_avx2(at, out) };
        }
        self.decode_body(at, out)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn decode_avx2(&self, at: usize, out: &mut [Frame]) -> bool {
        self.decode_body(at, out)
    }

    #[inline(always)]
    fn decode_body(&self, at: usize, out: &mut [Frame]) -> bool {
        let samples = 2 * at..2 * (at + out.len());
        let out = out.as_flattened_mut();
        match self {
            Self::F32(d) => match d.as_flattened().get(samples) {
                Some(s) => out.copy_from_slice(s),
                None => return false,
            },
            Self::I16(d) => match d.get(samples) {
                Some(s) => {
                    for (o, &s) in out.iter_mut().zip(s) {
                        *o = f32::from(s) * (1.0 / I16_SCALE);
                    }
                }
                None => return false,
            },
            Self::I24(high, low) => match (high.get(samples.clone()), low.get(samples)) {
                (Some(high), Some(low)) => {
                    for ((o, &h), &l) in out.iter_mut().zip(high).zip(low) {
                        *o = (i32::from(h) << 8 | i32::from(l)) as f32 * (1.0 / I24_SCALE);
                    }
                }
                _ => return false,
            },
        }
        true
    }
}

/// Whether `x * scale` is an integer in `-scale..scale`.
#[inline]
fn exact(x: f32, scale: f32) -> bool {
    let q = x * scale;
    ((q as i32) as f32 == q) & (q >= -scale) & (q < scale)
}

/// A fully decoded sample.
pub struct Sample {
    pub rate: u32,
    pub frames: Vec<Frame>,
}

/// Decode a whole sample, refusing anything longer than `max_frames`.
pub fn decode(path: &Path, max_frames: usize) -> Result<Sample> {
    let mut reader = Sources::default().source(path)?.open()?;
    ensure!(
        reader.frames <= max_frames as u64,
        "Sample exceeds remaining memory budget"
    );
    let mut frames = vec![[0.0; 2]; reader.frames as usize];
    reader.read(0, &mut frames)?;
    Ok(Sample {
        rate: reader.rate,
        frames,
    })
}

/// Where a sample's bytes live: a plain file or an archive member.
#[derive(Clone)]
pub struct Source {
    /// Virtual path; its extension selects the codec.
    path: PathBuf,
    file: PathBuf,
    offset: u64,
    len: Option<u64>,
    key: Option<Arc<LibraryKey>>,
}

/// Resolves sample paths, caching archive indexes and library keys.
#[derive(Default)]
pub struct Sources {
    archives: HashMap<PathBuf, (Archive, File)>,
    keys: HashMap<PathBuf, Option<Arc<LibraryKey>>>,
}

impl Sources {
    pub fn source(&mut self, path: &Path) -> Result<Source> {
        self.source_in(path, None)
    }

    /// Like [`Sources::source`]; `known` is the archive and member entry the
    /// importer already validated, which skips re-reading the archive index.
    pub fn source_in(&mut self, path: &Path, known: Option<&(PathBuf, Entry)>) -> Result<Source> {
        let (archive, entry) = match known {
            Some((archive, entry)) => (archive.clone(), entry.clone()),
            None => {
                let Some((archive, member)) = crate::import::archive_member(path) else {
                    return Ok(Source {
                        path: path.into(),
                        file: path.into(),
                        offset: 0,
                        len: None,
                        key: None,
                    });
                };
                if !self.archives.contains_key(&archive) {
                    let mut file = File::open(&archive)?;
                    let index = Archive::read_index(&mut file)
                        .with_context(|| format!("Archive {}", archive.display()))?;
                    self.archives.insert(archive.clone(), (index, file));
                }
                let (index, file) = &self.archives[&archive];
                let entry = index
                    .member(file, &member)?
                    .context("Archive member not found")?;
                (archive, entry)
            }
        };
        ensure!(
            entry.valid,
            "{}",
            entry.issue.unwrap_or("Invalid archive member")
        );
        let key = if entry.encoded && entry.key_index != 0xff {
            ensure!(entry.key_index == 0x100, "Unsupported legacy NKX cipher");
            if !self.keys.contains_key(&archive) {
                let key = crate::import::library_key(&archive)?.map(Arc::new);
                self.keys.insert(archive.clone(), key);
            }
            Some(
                self.keys[&archive]
                    .clone()
                    .context("Encrypted archive member needs local library access data")?,
            )
        } else {
            None
        };
        Ok(Source {
            path: path.into(),
            file: archive,
            offset: entry.offset,
            len: Some(entry.size),
            key,
        })
    }
}

impl Source {
    pub fn open(&self) -> Result<SampleReader> {
        SampleReader::open(self).with_context(|| {
            let why = if self.is_unwritten() {
                "library download is incomplete (its data is still zeros; repair it in Native Access)"
            } else {
                "decoding failed"
            };
            format!("{}: {why}", self.path.display())
        })
    }

    /// Whether the sample's first bytes on disk are all zero: an interrupted
    /// download preallocates archives and leaves the unfetched rest zeroed.
    fn is_unwritten(&self) -> bool {
        let mut head = [0u8; 64];
        let len = self.len.map_or(head.len() as u64, |l| l.min(head.len() as u64)) as usize;
        File::open(&self.file)
            .and_then(|mut f| {
                f.seek(SeekFrom::Start(self.offset))?;
                f.read_exact(&mut head[..len])
            })
            .is_ok_and(|()| len > 0 && head[..len].iter().all(|&b| b == 0))
    }

    fn bytes(&self) -> Result<Bytes> {
        let mut file = File::open(&self.file)
            .with_context(|| format!("Opening sample {}", self.file.display()))?;
        let len = match self.len {
            Some(len) => len,
            None => file.metadata()?.len(),
        };
        file.seek(SeekFrom::Start(self.offset))?;
        Ok(Bytes {
            file,
            base: self.offset,
            len,
            pos: 0,
            key: self.key.clone(),
        })
    }
}

/// A byte window of a file, decrypted on the fly when keyed.
struct Bytes {
    file: File,
    base: u64,
    len: u64,
    pos: u64,
    key: Option<Arc<LibraryKey>>,
}

impl Read for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let room = self.len.saturating_sub(self.pos).min(buf.len() as u64) as usize;
        let n = self.file.read(&mut buf[..room])?;
        if let Some(key) = &self.key {
            key.apply_at(self.pos, &mut buf[..n]);
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Bytes {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(n) => self.len.checked_add_signed(n),
            SeekFrom::Current(n) => self.pos.checked_add_signed(n),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        self.file.seek(SeekFrom::Start(self.base + pos))?;
        self.pos = pos;
        Ok(pos)
    }
}

impl MediaSource for Bytes {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// Random-access frame reader for WAV/AIFF (symphonia) and NCW (block codec).
pub struct SampleReader {
    pub rate: u32,
    pub frames: u64,
    /// Declared bits per sample.
    pub bits: Option<u16>,
    codec: Codec,
}

enum Codec {
    Ncw(Box<NcwCodec>),
    Pcm(Box<PcmCodec>),
}

struct NcwCodec {
    reader: ncw::NcwReader<BufReader<Bytes>>,
    channels: usize,
    float: bool,
    scale: f32,
    /// Cached decoded block: index and frames.
    block: Option<usize>,
    raw: Vec<i32>,
    frames: Vec<Frame>,
}

struct PcmCodec {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn codecs::Decoder>,
    track: u32,
    /// Decoded frames starting at sample frame `start`.
    buffer: Vec<Frame>,
    start: u64,
}

/// Forward gaps up to this many frames are decoded through instead of seeking.
const SKIP_AHEAD: u64 = 16384;

impl SampleReader {
    fn open(source: &Source) -> Result<Self> {
        let bytes = source.bytes()?;
        let ncw = source
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ncw"));
        let reader = if ncw {
            Self::ncw(bytes)?
        } else {
            Self::pcm(source, bytes)?
        };
        ensure!(
            reader.rate > 0 && reader.frames > 0,
            "Empty sample or invalid rate"
        );
        Ok(reader)
    }

    fn ncw(bytes: Bytes) -> Result<Self> {
        let reader = ncw::NcwReader::read(BufReader::new(bytes))?;
        let header = &reader.header;
        ensure!(
            (1..=2).contains(&header.channels),
            "Only mono/stereo samples are supported"
        );
        let (rate, frames) = (header.sample_rate, u64::from(header.num_samples));
        let bits = Some(header.bits_per_sample);
        let codec = NcwCodec {
            channels: header.channels as usize,
            scale: 2f32.powi(i32::from(header.bits_per_sample) - 1),
            float: reader.sample_format == ncw::SampleFormat::Float,
            reader,
            block: None,
            raw: Vec::new(),
            frames: Vec::new(),
        };
        Ok(Self {
            rate,
            frames,
            bits,
            codec: Codec::Ncw(Box::new(codec)),
        })
    }

    fn pcm(source: &Source, bytes: Bytes) -> Result<Self> {
        let mut hint = Hint::new();
        if let Some(extension) = source.path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(Box::new(bytes), Default::default());
        let format = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )?
            .format;
        let track = format.default_track().context("No audio track")?;
        let params = &track.codec_params;
        let frames = params.n_frames.context("Sample length is not declared")?;
        let rate = params.sample_rate.context("Sample rate is not declared")?;
        let bits = params.bits_per_sample.and_then(|b| u16::try_from(b).ok());
        if let Some(channels) = params.channels {
            ensure!(
                (1..=2).contains(&channels.count()),
                "Only mono/stereo samples are supported"
            );
        }
        let decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
        let track = track.id;
        let codec = PcmCodec {
            format,
            decoder,
            track,
            buffer: Vec::new(),
            start: 0,
        };
        Ok(Self {
            rate,
            frames,
            bits,
            codec: Codec::Pcm(Box::new(codec)),
        })
    }

    /// Fill `out` with frames from `start`. Frames past the end are silent;
    /// non-finite input decodes as silence.
    pub fn read(&mut self, start: u64, out: &mut [Frame]) -> Result<()> {
        let valid = self.frames.saturating_sub(start).min(out.len() as u64) as usize;
        let (head, tail) = out.split_at_mut(valid);
        tail.fill([0.0; 2]);
        match &mut self.codec {
            Codec::Ncw(codec) => codec.read(start, head)?,
            Codec::Pcm(codec) => codec.read(start, head)?,
        }
        for sample in head.as_flattened_mut() {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
        Ok(())
    }
}

impl NcwCodec {
    fn read(&mut self, mut start: u64, mut out: &mut [Frame]) -> Result<()> {
        const BLOCK: u64 = ncw::NcwReader::<BufReader<Bytes>>::FRAMES_PER_BLOCK as u64;
        while !out.is_empty() {
            let index = (start / BLOCK) as usize;
            if self.block != Some(index) {
                self.block = None;
                self.raw.clear();
                self.reader.decode_block_into(index, &mut self.raw)?;
                let (float, scale) = (self.float, self.scale);
                let convert = |s: i32| {
                    if float {
                        f32::from_bits(s as u32)
                    } else {
                        s as f32 / scale
                    }
                };
                let last = self.channels - 1;
                self.frames.clear();
                self.frames.extend(
                    self.raw
                        .chunks_exact(self.channels)
                        .map(|s| [convert(s[0]), convert(s[last])]),
                );
                self.block = Some(index);
            }
            let offset = (start % BLOCK) as usize;
            let available = self
                .frames
                .get(offset..)
                .context("NCW block shorter than declared")?;
            ensure!(!available.is_empty(), "NCW block shorter than declared");
            let n = available.len().min(out.len());
            out[..n].copy_from_slice(&available[..n]);
            out = &mut out[n..];
            start += n as u64;
        }
        Ok(())
    }
}

impl PcmCodec {
    fn end(&self) -> u64 {
        self.start + self.buffer.len() as u64
    }

    fn read(&mut self, mut start: u64, mut out: &mut [Frame]) -> Result<()> {
        if start < self.start || start > self.end() + SKIP_AHEAD {
            self.seek(start)?;
        }
        while !out.is_empty() {
            if start < self.end() && start >= self.start {
                let offset = (start - self.start) as usize;
                let n = (self.buffer.len() - offset).min(out.len());
                out[..n].copy_from_slice(&self.buffer[offset..offset + n]);
                out = &mut out[n..];
                start += n as u64;
            } else if start < self.start {
                bail!("Sample seek landed past frame {start}");
            } else if !self.decode_next()? {
                bail!("Sample ended before its declared length");
            }
        }
        Ok(())
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        let to = SeekTo::TimeStamp {
            ts: frame,
            track_id: self.track,
        };
        let seeked = self.format.seek(SeekMode::Accurate, to)?;
        self.decoder.reset();
        self.buffer.clear();
        self.start = seeked.actual_ts;
        Ok(())
    }

    /// Decode the next packet into the buffer; false at end of stream.
    fn decode_next(&mut self) -> Result<bool> {
        let packet = loop {
            match self.format.next_packet() {
                Ok(packet) if packet.track_id() == self.track => break packet,
                Ok(_) => continue,
                Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            }
        };
        let decoded = self.decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        ensure!(
            (1..=2).contains(&channels),
            "Only mono/stereo samples are supported"
        );
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        pcm.copy_interleaved_ref(decoded);
        self.start = packet.ts();
        self.buffer.clear();
        self.buffer.extend(
            pcm.samples()
                .chunks_exact(channels)
                .map(|s| [s[0], s[channels - 1]]),
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_stores_each_resolution_exactly() {
        let at = |scale: f32| move |i: i32| (i * 7919 % 65536 - 32768) as f32 * scale;
        let i16 = at(1.0 / 32768.0);
        let i24 = at(1.0 / 8388608.0 * 255.0);
        let cases: [(Vec<Frame>, usize); 3] = [
            ((0..1000).map(|i| [i16(i), i16(i + 1)]).collect(), 4),
            ((0..1000).map(|i| [i24(i), -i24(i)]).collect(), 6),
            ((0..1000).map(|i| [(i as f32).sin(), 1.0]).collect(), 8),
        ];
        for (frames, width) in cases {
            let pcm = Pcm::pack(&frames);
            assert_eq!(pcm.bytes(), width * frames.len());
            let mut out = vec![[0.0; 2]; 900];
            assert_eq!(pcm.window(100, &mut out).unwrap(), &frames[100..]);
            assert!(pcm.window(101, &mut out).is_none());
        }
    }
}
