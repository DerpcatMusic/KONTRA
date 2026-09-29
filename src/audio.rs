//! Sample decoding with random access by frame.
//!
//! Every format goes through [`SampleReader`], so the loader decodes only the
//! resident spans of a sample and the streamer fetches the rest on demand.
//! Archive members are decrypted in place while reading: the NKX resource
//! cipher is position-relative, so any byte offset is addressable.

use anyhow::{Context, Result, bail, ensure};
use ni_file::{nis::LibraryKey, nkr::Archive};
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

/// One stereo frame. Mono sources are duplicated to both channels.
pub type Frame = [f32; 2];

/// A fully decoded sample.
pub struct Sample {
    pub rate: u32,
    pub frames: Vec<Frame>,
}

/// Decode a whole sample, refusing anything longer than `max_frames`.
pub fn decode(path: &Path, max_frames: usize) -> Result<Sample> {
    let mut reader = Sources::default().source(path)?.open()?;
    ensure!(reader.frames <= max_frames as u64, "Sample exceeds remaining memory budget");
    let mut frames = vec![[0.0; 2]; reader.frames as usize];
    reader.read(0, &mut frames)?;
    Ok(Sample { rate: reader.rate, frames })
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
    archives: HashMap<PathBuf, (Archive, Option<Arc<LibraryKey>>)>,
}

impl Sources {
    pub fn source(&mut self, path: &Path) -> Result<Source> {
        let Some((archive, member)) = crate::import::archive_member(path) else {
            return Ok(Source { path: path.into(), file: path.into(), offset: 0, len: None, key: None });
        };
        if !self.archives.contains_key(&archive) {
            let index = Archive::read(File::open(&archive)?)
                .with_context(|| format!("Archive {}", archive.display()))?;
            let key = crate::import::library_key(&archive)?.map(Arc::new);
            self.archives.insert(archive.clone(), (index, key));
        }
        let (index, key) = &self.archives[&archive];
        let entry = index.find(&member).context("Archive member not found")?;
        ensure!(entry.valid, "{}", entry.issue.unwrap_or("Invalid archive member"));
        let key = if entry.encoded && entry.key_index != 0xff {
            ensure!(entry.key_index == 0x100, "Unsupported legacy NKX cipher");
            Some(key.clone().context("Encrypted archive member needs local library access data")?)
        } else {
            None
        };
        Ok(Source { path: path.into(), file: archive, offset: entry.offset, len: Some(entry.size), key })
    }
}

impl Source {
    pub fn open(&self) -> Result<SampleReader> {
        SampleReader::open(self).with_context(|| format!("Decoding {}", self.path.display()))
    }

    fn bytes(&self) -> Result<Bytes> {
        let mut file =
            File::open(&self.file).with_context(|| format!("Opening sample {}", self.file.display()))?;
        let len = match self.len {
            Some(len) => len,
            None => file.metadata()?.len(),
        };
        file.seek(SeekFrom::Start(self.offset))?;
        Ok(Bytes { file, base: self.offset, len, pos: 0, key: self.key.clone() })
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
        let ncw = source.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ncw"));
        let reader = if ncw { Self::ncw(bytes)? } else { Self::pcm(source, bytes)? };
        ensure!(reader.rate > 0 && reader.frames > 0, "Empty sample or invalid rate");
        Ok(reader)
    }

    fn ncw(bytes: Bytes) -> Result<Self> {
        let reader = ncw::NcwReader::read(BufReader::new(bytes))?;
        let header = &reader.header;
        ensure!((1..=2).contains(&header.channels), "Only mono/stereo samples are supported");
        let (rate, frames) = (header.sample_rate, u64::from(header.num_samples));
        let codec = NcwCodec {
            channels: header.channels as usize,
            scale: 2f32.powi(i32::from(header.bits_per_sample) - 1),
            float: reader.sample_format == ncw::SampleFormat::Float,
            reader,
            block: None,
            raw: Vec::new(),
            frames: Vec::new(),
        };
        Ok(Self { rate, frames, codec: Codec::Ncw(Box::new(codec)) })
    }

    fn pcm(source: &Source, bytes: Bytes) -> Result<Self> {
        let mut hint = Hint::new();
        if let Some(extension) = source.path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(Box::new(bytes), Default::default());
        let format = symphonia::default::get_probe()
            .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())?
            .format;
        let track = format.default_track().context("No audio track")?;
        let params = &track.codec_params;
        let frames = params.n_frames.context("Sample length is not declared")?;
        let rate = params.sample_rate.context("Sample rate is not declared")?;
        if let Some(channels) = params.channels {
            ensure!((1..=2).contains(&channels.count()), "Only mono/stereo samples are supported");
        }
        let decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
        let track = track.id;
        let codec = PcmCodec { format, decoder, track, buffer: Vec::new(), start: 0 };
        Ok(Self { rate, frames, codec: Codec::Pcm(Box::new(codec)) })
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
                let convert = |s: i32| if float { f32::from_bits(s as u32) } else { s as f32 / scale };
                let last = self.channels - 1;
                self.frames.clear();
                self.frames.extend(self.raw.chunks_exact(self.channels).map(|s| [convert(s[0]), convert(s[last])]));
                self.block = Some(index);
            }
            let offset = (start % BLOCK) as usize;
            let available = self.frames.get(offset..).context("NCW block shorter than declared")?;
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
        let to = SeekTo::TimeStamp { ts: frame, track_id: self.track };
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
        ensure!((1..=2).contains(&channels), "Only mono/stereo samples are supported");
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        pcm.copy_interleaved_ref(decoded);
        self.start = packet.ts();
        self.buffer.clear();
        self.buffer.extend(pcm.samples().chunks_exact(channels).map(|s| [s[0], s[channels - 1]]));
        Ok(true)
    }
}
