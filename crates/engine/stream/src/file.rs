//! Sample files as the store reads them: PCM straight from disk, NCW decoded
//! per block. Where the bytes are is the format's job ([`ir::Sample`]).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;

use kontra_ir as ir;

use crate::{Encoding, SampleFmt, Source};

/// Opens `sample` for the store: its source and the packed layout it reads as.
///
/// # Errors
/// When the file cannot be opened, or its codec or layout is not supported.
pub fn open(sample: &ir::Sample) -> io::Result<(Arc<dyn Source>, SampleFmt)> {
    let file = Arc::new(File::open(&sample.file)?);
    let region = Region {
        file,
        base: sample.offset,
        len: sample.len,
        keystream: sample.keystream.clone(),
    };
    match sample.codec {
        ir::Codec::Pcm {
            bits,
            float,
            big_endian,
        } => {
            let (encoding, convert) = match (bits, float, big_endian) {
                (16, false, false) => (Encoding::I16, None),
                (24, false, false) => (Encoding::I24, None),
                (32, true, false) => (Encoding::F32, None),
                (8 | 16 | 24 | 32, false, _) | (32, true, true) => (
                    Encoding::F32,
                    Some(Pcm {
                        bits,
                        float,
                        big_endian,
                    }),
                ),
                _ => return Err(unsupported(format!("{bits}-bit PCM"))),
            };
            let fmt = SampleFmt {
                encoding,
                channels: sample.channels,
            };
            let source = PcmSource {
                region,
                width: usize::from(bits / 8),
                channels: usize::from(sample.channels),
                convert,
            };
            Ok((Arc::new(source), fmt))
        }
        ir::Codec::Ncw => {
            let reader = ncw::NcwReader::read(region.reader()).map_err(invalid)?;
            let header = reader.header.clone();
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
                encoding,
                format: reader.sample_format,
                offsets: reader.block_offsets.into(),
                header,
            };
            Ok((Arc::new(source), SampleFmt { encoding, channels }))
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

#[derive(Clone, Copy)]
struct Pcm {
    bits: u8,
    float: bool,
    big_endian: bool,
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
    encoding: Encoding,
    format: ncw::SampleFormat,
    header: ncw::NcwHeader,
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
            .unwrap_or(self.header.data_size);
        (start <= end).then_some((start, end))
    }

    fn write(&self, value: i32, dest: &mut [u8]) {
        match self.encoding {
            Encoding::I16 | Encoding::I24 => {
                let width = dest.len();
                dest.copy_from_slice(value.to_le_bytes().get(..width).unwrap_or_default());
            }
            Encoding::F32 => {
                let v = if self.format == ncw::SampleFormat::Float {
                    f32::from_bits(value as u32)
                } else {
                    value as f32 / 2f32.powi(i32::from(self.header.bits_per_sample) - 1)
                };
                dest.copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

impl Source for NcwSource {
    fn read(&self, first: u64, out: &mut [u8]) -> io::Result<()> {
        out.fill(0);
        let channels = usize::from(self.header.channels);
        let width = self.encoding.bytes();
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
            u64::from(self.header.data_offset) + u64::from(from),
            &mut bytes,
        )?;
        let mut decoded = Vec::new();
        for index in blocks {
            let (start, end) = self.group(index).unwrap_or((from, from));
            let mut group = bytes
                .get((start - from) as usize..(end - from) as usize)
                .unwrap_or_default();
            ncw::decode_group(&mut group, &self.header, self.format, index, &mut decoded)
                .map_err(invalid)?;
            let block_first = index as u64 * per_block;
            for (c, samples) in decoded.iter().enumerate() {
                for (n, &value) in samples.iter().enumerate() {
                    let at = block_first + n as u64;
                    if at < first || at >= first + frames {
                        continue;
                    }
                    let offset = (at - first) as usize * frame + c * width;
                    if let Some(dest) = out.get_mut(offset..offset + width) {
                        self.write(value, dest);
                    }
                }
            }
        }
        Ok(())
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

    fn sample(
        file: std::path::PathBuf,
        offset: u64,
        len: u64,
        codec: ir::Codec,
        frames: u64,
    ) -> ir::Sample {
        ir::Sample {
            name: "t".into(),
            file,
            offset,
            len,
            codec,
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
        let s = sample(path, 100, bytes.len() as u64, ir::Codec::Ncw, 1500);
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
    fn big_endian_pcm_is_converted_to_float() {
        let frames: Vec<u8> = [0x40i16, 0x00, -0x4000, 0x2000]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let path = temp("be.aif", &frames);
        let codec = ir::Codec::Pcm {
            bits: 16,
            float: false,
            big_endian: true,
        };
        let (source, fmt) = open(&sample(path, 0, 8, codec, 2)).unwrap();
        assert_eq!(fmt.encoding, Encoding::F32);
        let mut out = vec![0; 16];
        source.read(1, &mut out).unwrap();
        let v = f32::from_le_bytes([out[0], out[1], out[2], out[3]]);
        assert_eq!(v, -0.5);
    }
}
