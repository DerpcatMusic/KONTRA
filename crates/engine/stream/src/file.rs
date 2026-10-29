//! Sample files as the store reads them: PCM straight from disk, NCW decoded
//! per block, FLAC decoded per read. Where the bytes are is the format's job ([`ir::Sample`]).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_ir as ir;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::default::codecs::FlacDecoder;
use symphonia::default::formats::FlacReader;

use crate::sniff::{Layout, Pcm, sniff};
use crate::source::Packing;
use crate::{Encoding, SampleFmt, Source};

/// Opens `sample` for the store: its source and the packed layout it reads as.
///
/// # Errors
/// When the file cannot be opened, or its header names a layout this cannot read.
pub fn open(sample: &ir::Sample) -> io::Result<(Arc<dyn Source>, SampleFmt)> {
    opened(sample).map(|o| (o.source, o.fmt))
}

/// The files an instrument's samples live in, each opened once: a monolith
/// or bank holds thousands of samples, and each opening its own handle
/// opened the file thousands of times per load and kept as many open.
pub struct Files(HashMap<PathBuf, Arc<File>>);

impl Files {
    /// Opens each of `paths` once; one that fails is opened again, and its
    /// error reported, by the sample that asks for it.
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut files = HashMap::new();
        for path in paths {
            if !files.contains_key(path)
                && let Ok(file) = random_access(path)
            {
                files.insert(path.to_path_buf(), Arc::new(file));
            }
        }
        Self(files)
    }

    /// [`open`], reading through the file's shared handle. Safe from
    /// several loader threads at once.
    ///
    /// # Errors
    /// As [`open`].
    pub fn open(&self, sample: &ir::Sample) -> io::Result<(Arc<dyn Source>, SampleFmt)> {
        let file = match self.0.get(&sample.file) {
            Some(file) => Arc::clone(file),
            None => Arc::new(random_access(&sample.file)?),
        };
        opened_in(sample, file).map(|o| (o.source, o.fmt))
    }
}

/// Opens `path` for a preload's short, scattered reads.
fn random_access(path: &Path) -> io::Result<File> {
    let file = File::open(path)?;
    // Preloads read short, scattered ranges: no read-ahead until the
    // preload is in ([`Region::loaded`]). With it the kernel read 7.3 GB
    // from disk to pin 3.0 GB of David And Goliath's heads; without, 3.1 GB.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let _ = rustix::fs::fadvise(&file, 0, None, rustix::fs::Advice::Random);
    Ok(file)
}

/// The whole of `sample` as stereo frames, and its rate; for short files
/// read once (convolution responses), never for streaming.
///
/// # Errors
/// As [`open`], and when the header names no rate or the file is longer than `max_frames`.
pub fn decode(sample: &ir::Sample, max_frames: u64) -> io::Result<(u32, Vec<[f32; 2]>)> {
    let o = opened(sample)?;
    // 64 MiB holds stereo f32 at any `max_frames` a response asks for.
    const MAX_BYTES: usize = 64 << 20;
    if o.rate == 0 || o.frames == 0 || o.frames > max_frames {
        return Err(unsupported(format!("{} frames at {} Hz", o.frames, o.rate)));
    }
    let frames = usize::try_from(o.frames).map_err(|_| unsupported("length".into()))?;
    let size = frames
        .checked_mul(o.fmt.bytes_per_frame())
        .filter(|&n| n <= MAX_BYTES)
        .ok_or_else(|| unsupported(format!("{frames} frames of {:?}", o.fmt)))?;
    let mut bytes = vec![0; size];
    o.source.read(0, &mut bytes)?;
    let mut out = vec![[0.0; 2]; frames];
    o.fmt
        .decode(|i| bytes.get(i).copied().unwrap_or(0), 0, &mut out, None);
    Ok((o.rate, out))
}

struct Opened {
    source: Arc<dyn Source>,
    fmt: SampleFmt,
    rate: u32,
    frames: u64,
}

fn opened(sample: &ir::Sample) -> io::Result<Opened> {
    opened_in(sample, Arc::new(random_access(&sample.file)?))
}

fn opened_in(sample: &ir::Sample, file: Arc<File>) -> io::Result<Opened> {
    let len = match sample.len {
        Some(len) => len,
        None => file.metadata()?.len().saturating_sub(sample.offset),
    };
    let region = Region {
        file,
        base: sample.offset,
        len,
        keystream: sample.keystream.clone(),
    };
    let size = region.len;
    match sniff(size, |pos, out| region.read_at(pos, out))? {
        Layout::Pcm {
            offset,
            len,
            channels,
            pcm,
            rate,
        } => {
            let (encoding, convert) = match pcm {
                Pcm {
                    bits: 16,
                    float: false,
                    big_endian: false,
                } => (Encoding::I16, None),
                Pcm {
                    bits: 24,
                    float: false,
                    big_endian: false,
                } => (Encoding::I24, None),
                Pcm {
                    bits: 32,
                    float: true,
                    big_endian: false,
                } => (Encoding::F32, None),
                Pcm {
                    bits: 8 | 16 | 24 | 32,
                    float: false,
                    ..
                }
                | Pcm {
                    bits: 32,
                    float: true,
                    ..
                } => (Encoding::F32, Some(pcm)),
                Pcm { bits, .. } => return Err(unsupported(format!("{bits}-bit PCM"))),
            };
            let source = PcmSource {
                region: Region {
                    base: region.base + offset,
                    len,
                    ..region
                },
                width: usize::from(pcm.bits / 8),
                channels: usize::from(channels),
                convert,
            };
            let frame = u64::from(pcm.bits / 8) * u64::from(channels);
            Ok(Opened {
                source: Arc::new(source),
                fmt: SampleFmt { encoding, channels },
                rate,
                frames: len / frame.max(1),
            })
        }
        Layout::Flac => {
            let (reader, _) = FlacSource::open(&region)?;
            let params = &reader
                .default_track()
                .ok_or_else(|| unsupported("FLAC without a track".into()))?
                .codec_params;
            let channels = params.channels.map_or(0, |c| c.count());
            let encoding = match params.bits_per_sample {
                Some(16) => Encoding::I16,
                Some(24) => Encoding::I24,
                _ => Encoding::F32,
            };
            let source = FlacSource {
                region,
                encoding,
                channels,
            };
            let channels = u8::try_from(channels).map_err(|_| unsupported("channels".into()))?;
            Ok(Opened {
                source: Arc::new(source),
                fmt: SampleFmt { encoding, channels },
                rate: params.sample_rate.unwrap_or(0),
                frames: params.n_frames.unwrap_or(0),
            })
        }
        Layout::Ncw => {
            // Buffered: the offset table is read four bytes at a time. 4 KiB
            // holds the header and the table of a typical sample (4 bytes a
            // block of 512 frames); a 64 KiB buffer read 1.8 GB more on a
            // 28,000-sample load than the heads it pinned.
            let buffered = io::BufReader::with_capacity(4 << 10, region.reader());
            let reader = ncw::NcwReader::read(buffered).map_err(invalid)?;
            let header = reader.header.clone();
            let (rate, frames) = (header.sample_rate, u64::from(header.num_samples));
            let float = reader.sample_format == ncw::SampleFormat::Float;
            let encoding = match (header.bits_per_sample, float) {
                (16, false) => Encoding::I16,
                (24, false) => Encoding::I24,
                _ => Encoding::F32,
            };
            let channels =
                u8::try_from(header.channels).map_err(|_| unsupported("channels".into()))?;
            let source = NcwSource {
                region,
                packing: Packing {
                    header,
                    format: reader.sample_format,
                    encoding,
                },
                offsets: reader.block_offsets.into(),
            };
            Ok(Opened {
                source: Arc::new(source),
                fmt: SampleFmt { encoding, channels },
                rate,
                frames,
            })
        }
    }
}

fn invalid(e: ncw::NcwError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

fn unsupported(what: String) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, what)
}

/// A byte range of a shared file, read by position so readers never share a cursor.
#[derive(Clone)]
struct Region {
    file: Arc<File>,
    base: u64,
    len: u64,
    keystream: Option<ir::Keystream>,
}

impl Region {
    /// Reads from `at` into `out`; bytes past the region are zero.
    fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<()> {
        let available = usize::try_from(self.len.saturating_sub(at)).unwrap_or(usize::MAX);
        let (head, tail) = out.split_at_mut(available.min(out.len()));
        tail.fill(0);
        let mut done = 0;
        while let Some(rest) = head.get_mut(done..).filter(|r| !r.is_empty()) {
            let n = read_at(&self.file, rest, self.base + at + done as u64)?;
            if n == 0 {
                rest.fill(0);
                break;
            }
            done += n;
        }
        if let Some(key) = &self.keystream {
            key.apply(self.base + at, head);
        }
        Ok(())
    }

    /// The preload is in: streaming reads in order, so read-ahead helps again.
    fn loaded(&self) {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let _ = rustix::fs::fadvise(&*self.file, 0, None, rustix::fs::Advice::Normal);
    }

    fn reader(&self) -> RegionReader {
        RegionReader {
            region: self.clone(),
            pos: 0,
        }
    }
}

#[cfg(unix)]
fn read_at(file: &File, out: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, out, at)
}

#[cfg(windows)]
fn read_at(file: &File, out: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, out, at)
}

impl symphonia::core::io::MediaSource for RegionReader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.region.len)
    }
}

/// `Read + Seek` over a region, for decoders that want a stream.
struct RegionReader {
    region: Region,
    pos: u64,
}

impl Read for RegionReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let left = usize::try_from(self.region.len.saturating_sub(self.pos)).unwrap_or(usize::MAX);
        let n = left.min(out.len());
        let out = out.get_mut(..n).unwrap_or_default();
        self.region.read_at(self.pos, out)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for RegionReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.region.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = pos.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.pos)
    }
}

impl Pcm {
    fn value(self, b: &[u8]) -> f32 {
        let mut raw = [0u8; 4];
        let width = usize::from(self.bits / 8);
        for (i, byte) in b.iter().take(width).enumerate() {
            // Little endian, the top byte last; big endian reversed.
            let at = if self.big_endian { width - 1 - i } else { i };
            if let Some(slot) = raw.get_mut(at + 4 - width) {
                *slot = *byte;
            }
        }
        let word = i32::from_le_bytes(raw);
        if self.float {
            f32::from_bits(word as u32)
        } else if self.bits == 8 && !self.big_endian {
            // WAV 8-bit is unsigned.
            ((word >> 24) as u8 as f32 - 128.0) / 128.0
        } else {
            word as f32 / 2_147_483_648.0
        }
    }
}

struct PcmSource {
    region: Region,
    width: usize,
    channels: usize,
    /// Set when the stored layout is not one the store packs: converted to f32.
    convert: Option<Pcm>,
}

impl Source for PcmSource {
    fn loaded(&self) {
        self.region.loaded();
    }

    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()> {
        let frame = (self.width * self.channels) as u64;
        let Some(pcm) = self.convert else {
            return self.region.read_at(first * frame, out);
        };
        let values = out.len() / 4;
        let mut raw = vec![0; values * self.width];
        self.region.read_at(first * frame, &mut raw)?;
        for (dest, src) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(raw.chunks_exact(self.width))
        {
            dest.copy_from_slice(&pcm.value(src).to_le_bytes());
        }
        Ok(())
    }
}

/// NCW decoded per block group. The block table is read once at open; a read
/// fetches the bytes of every group it touches in one positional read.
struct NcwSource {
    region: Region,
    packing: Packing,
    offsets: Box<[u32]>,
}

impl NcwSource {
    /// Byte range of group `index` relative to the data start.
    fn group(&self, index: usize) -> Option<(u32, u32)> {
        let start = *self.offsets.get(index)?;
        let end = self
            .offsets
            .get(index + 1)
            .copied()
            .unwrap_or(self.packing.header.data_size);
        (start <= end).then_some((start, end))
    }

    fn write(&self, value: i32, dest: &mut [u8]) {
        match self.packing.encoding {
            Encoding::I16 | Encoding::I24 => {
                let width = dest.len();
                dest.copy_from_slice(value.to_le_bytes().get(..width).unwrap_or_default());
            }
            Encoding::F32 => {
                let v = if self.packing.format == ncw::SampleFormat::Float {
                    f32::from_bits(value as u32)
                } else {
                    value as f32 / 2f32.powi(i32::from(self.packing.header.bits_per_sample) - 1)
                };
                dest.copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

impl Source for NcwSource {
    fn loaded(&self) {
        self.region.loaded();
    }

    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()> {
        out.fill(0);
        let channels = usize::from(self.packing.header.channels);
        let width = self.packing.encoding.bytes();
        let frame = width * channels;
        let frames = (out.len() / frame.max(1)) as u64;
        let per_block = ncw::SAMPLES_PER_BLOCK as u64;
        let last = (first + frames)
            .div_ceil(per_block)
            .min(self.offsets.len() as u64);
        let blocks = (first / per_block) as usize..last as usize;
        let (Some((from, _)), Some((_, to))) = (
            self.group(blocks.start),
            self.group(blocks.end.saturating_sub(1)),
        ) else {
            return Ok(());
        };
        let mut bytes = vec![0; (to - from) as usize];
        self.region.read_at(
            u64::from(self.packing.header.data_offset) + u64::from(from),
            &mut bytes,
        )?;
        let mut decoded = Vec::new();
        for index in blocks {
            let (start, end) = self.group(index).unwrap_or((from, from));
            let mut group = bytes
                .get((start - from) as usize..(end - from) as usize)
                .unwrap_or_default();
            ncw::decode_group(
                &mut group,
                &self.packing.header,
                self.packing.format,
                index,
                &mut decoded,
            )
            .map_err(invalid)?;
            // The part of this block the caller asked for, interleaved into `out`.
            let block_first = index as u64 * per_block;
            let skip = first.saturating_sub(block_first) as usize;
            let start = block_first.max(first) - first;
            for (c, samples) in decoded.iter().enumerate() {
                let at = start as usize * frame + c * width;
                let dest = out.get_mut(at..).unwrap_or_default().chunks_mut(frame);
                for (&value, d) in samples.get(skip..).unwrap_or_default().iter().zip(dest) {
                    if let Some(d) = d.get_mut(..width) {
                        self.write(value, d);
                    }
                }
            }
        }
        Ok(())
    }

    fn packing(&self) -> Option<&Packing> {
        (self.packing.header.channels <= 2).then_some(&self.packing)
    }

    fn read_blocks(&self, blocks: Range<usize>) -> io::Result<(Vec<u8>, Vec<u32>)> {
        let last = blocks.end.saturating_sub(1);
        let (Some((from, _)), Some((_, to))) = (self.group(blocks.start), self.group(last)) else {
            return Err(io::ErrorKind::UnexpectedEof.into());
        };
        let mut bytes = vec![0; (to - from) as usize];
        self.region.read_at(
            u64::from(self.packing.header.data_offset) + u64::from(from),
            &mut bytes,
        )?;
        let starts = blocks
            .map(|i| self.group(i).map_or(0, |(start, _)| start - from))
            .chain([to - from])
            .collect();
        Ok((bytes, starts))
    }
}

/// FLAC, as UVI banks store it in `.flac`, `.wav` and `.aif` members. A read
/// opens the stream, seeks to the frame asked for and decodes only what it
/// covers.
// ponytail: each read re-reads the metadata and binary-searches for the
// frame; keep a decoder per sample if FLAC page reads show in profiles.
struct FlacSource {
    region: Region,
    encoding: Encoding,
    channels: usize,
}

impl FlacSource {
    fn open(region: &Region) -> io::Result<(FlacReader, u32)> {
        let stream = MediaSourceStream::new(Box::new(region.reader()), Default::default());
        let reader = FlacReader::try_new(stream, &FormatOptions::default()).map_err(flac)?;
        let track = reader.default_track().map_or(0, |t| t.id);
        Ok((reader, track))
    }
}

fn flac(e: symphonia::core::errors::Error) -> io::Error {
    match e {
        symphonia::core::errors::Error::IoError(e) => e,
        e => io::Error::new(io::ErrorKind::InvalidData, e.to_string()),
    }
}

impl Source for FlacSource {
    fn loaded(&self) {
        self.region.loaded();
    }

    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()> {
        out.fill(0);
        let width = self.encoding.bytes();
        let frame = width * self.channels;
        let end = first + (out.len() / frame.max(1)) as u64;
        let (mut reader, track) = Self::open(&self.region)?;
        let params = reader
            .default_track()
            .map(|t| t.codec_params.clone())
            .unwrap_or_default();
        let mut decoder =
            FlacDecoder::try_new(&params, &DecoderOptions::default()).map_err(flac)?;
        let to = SeekTo::TimeStamp {
            ts: first,
            track_id: track,
        };
        if reader.seek(SeekMode::Accurate, to).is_err() {
            // Past the end: the tail stays silent.
            return Ok(());
        }
        loop {
            let packet = match reader.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(());
                }
                Err(e) => return Err(flac(e)),
            };
            if packet.ts >= end {
                return Ok(());
            }
            let AudioBufferRef::S32(buf) = decoder.decode(&packet).map_err(flac)? else {
                return Err(unsupported("FLAC decoded to a non-i32 buffer".into()));
            };
            // The packet's frames inside `first..end`, then one channel at a time.
            let skip = first.saturating_sub(packet.ts) as usize;
            let at = packet.ts.saturating_sub(first) as usize * frame;
            let Some(dest) = out.get_mut(at..) else {
                continue;
            };
            for c in 0..self.channels.min(buf.spec().channels.count()) {
                let (values, at) = (buf.chan(c).get(skip..).unwrap_or_default(), c * width);
                match self.encoding {
                    Encoding::I16 => put(dest, frame, at, values, |v| {
                        ((v >> 16) as i16).to_le_bytes()
                    }),
                    Encoding::I24 => put(dest, frame, at, values, |v| {
                        let [_, a, b, c] = v.to_le_bytes();
                        [a, b, c]
                    }),
                    Encoding::F32 => put(dest, frame, at, values, |v| {
                        (v as f32 / 2_147_483_648.0).to_le_bytes()
                    }),
                }
            }
        }
    }
}

/// Writes full-scale `i32`s into one channel of interleaved frames: the top
/// bytes for integer layouts, a float in -1..1 otherwise.
fn put<const N: usize>(
    dest: &mut [u8],
    frame: usize,
    at: usize,
    values: &[i32],
    bytes: impl Fn(i32) -> [u8; N],
) {
    for (f, &v) in dest.chunks_exact_mut(frame).zip(values) {
        if let Some(d) = f.get_mut(at..).and_then(|d| d.first_chunk_mut::<N>()) {
            *d = bytes(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("kontra-stream-file-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn sample(file: std::path::PathBuf, offset: u64, len: Option<u64>, frames: u64) -> ir::Sample {
        ir::Sample {
            name: "t".into(),
            file,
            offset,
            len,
            channels: 2,
            frames,
            rate: 48_000,
            keystream: None,
        }
    }

    #[test]
    fn ncw_decodes_the_frames_asked_for_at_any_offset() {
        let pcm: Vec<i32> = (0..1500).flat_map(|i| [i * 3, -i * 3]).collect();
        let spec = ncw::PcmSpec {
            channels: 2,
            bits_per_sample: 16,
            sample_rate: 48_000,
        };
        let bytes = ncw::encode_pcm(&pcm, spec, ncw::StereoMode::Direct).unwrap();
        let mut monolith = vec![0xAA; 100];
        monolith.extend(&bytes);
        let path = temp("a.ncw", &monolith);
        let s = sample(path, 100, Some(bytes.len() as u64), 1500);
        let (source, fmt) = open(&s).unwrap();
        assert_eq!(
            fmt,
            SampleFmt {
                encoding: Encoding::I16,
                channels: 2
            }
        );
        let mut out = vec![0; 10 * 4];
        source.read(1020, &mut out).unwrap();
        let first = i16::from_le_bytes([out[0], out[1]]);
        let right = i16::from_le_bytes([out[2], out[3]]);
        assert_eq!(
            (first, right),
            (3060, -3060),
            "frame 1020 crosses no block edge wrongly"
        );
        let last = i16::from_le_bytes([out[36], out[37]]);
        assert_eq!(last, 3 * 1029);
    }

    #[test]
    fn big_endian_aiff_is_converted_to_float() {
        let mut aiff = b"FORM\0\0\0\0AIFF".to_vec();
        aiff.extend(b"COMM\0\0\0\x12");
        aiff.extend(2u16.to_be_bytes());
        aiff.extend(2u32.to_be_bytes());
        aiff.extend(16u16.to_be_bytes());
        aiff.extend([0; 10]);
        aiff.extend(b"SSND\0\0\0\x10");
        aiff.extend([0; 8]);
        aiff.extend(
            [0x40i16, 0x00, -0x4000, 0x2000]
                .iter()
                .flat_map(|v| v.to_be_bytes()),
        );
        let (source, fmt) = open(&sample(temp("be.aif", &aiff), 0, None, 2)).unwrap();
        assert_eq!(fmt.encoding, Encoding::F32);
        let mut out = vec![0; 16];
        source.read(1, &mut out).unwrap();
        let v = f32::from_le_bytes([out[0], out[1], out[2], out[3]]);
        assert_eq!(v, -0.5);
    }

    /// A 10,000-frame stereo ramp (left `n`, right `-n`) in 1,024-frame FLAC
    /// blocks, stored after 100 bytes of something else, as in a UFS bank.
    #[test]
    fn flac_decodes_the_frames_asked_for_across_blocks() {
        let flac = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/ramp-16-stereo.flac"
        ))
        .unwrap();
        let mut bank = vec![0xAA; 100];
        bank.extend(&flac);
        let len = Some(flac.len() as u64);
        let (source, fmt) = open(&sample(temp("ramp.ufs", &bank), 100, len, 10_000)).unwrap();
        assert_eq!((fmt.encoding, fmt.channels), (Encoding::I16, 2));
        let mut out = vec![0; 2_000 * 4];
        source.read(1_000, &mut out).unwrap();
        for (i, f) in out.as_chunks::<4>().0.iter().enumerate() {
            let n = 1_000 + i as i16;
            assert_eq!(
                [
                    i16::from_le_bytes([f[0], f[1]]),
                    i16::from_le_bytes([f[2], f[3]])
                ],
                [n, -n]
            );
        }
        // Past the end stays silent.
        source.read(9_990, &mut out).unwrap();
        assert!(out[10 * 4..].iter().all(|&b| b == 0));
    }
}
