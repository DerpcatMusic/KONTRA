//! Pure-Rust lightweight audio header and loop prober for WAV (RIFF/RF64),
//! FLAC, AIFF/AIFC, and OGG/Vorbis.
//!
//! Reads only container headers and loop metadata using bounded, seek-based
//! I/O — never decoding audio frames or allocating proportional to file size.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use kontra_ir as ir;

/// Metadata extracted from an audio file header without decoding sample data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioHeader {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub loops: Vec<AudioLoop>,
}

/// An embedded sample loop point read from a file's metadata (`smpl` or `INST`/`MARK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioLoop {
    /// First frame included in the loop (inclusive).
    pub start: u64,
    /// First frame past the loop (exclusive), clamped to [`AudioHeader::frames`].
    pub end: u64,
    /// Repeats after the first traversal (`0` means repeat forever).
    pub count: u32,
    /// Alternates forward and backward on each repeat.
    pub ping_pong: bool,
}

impl AudioLoop {
    /// Converts this probed loop into an IR [`ir::Loop`].
    #[must_use]
    pub const fn to_ir_loop(self, until_release: bool) -> ir::Loop {
        ir::Loop {
            start: self.start,
            end: self.end,
            crossfade: 0,
            count: self.count,
            alternating: self.ping_pong,
            until_release,
        }
    }
}

/// Bounds on metadata chunk scanning so malformed files cannot loop or OOM.
const MAX_WAV_PRE_DATA_CHUNKS: usize = 8192;
const MAX_METADATA_CHUNKS: usize = 256;
const MAX_INLINE_LOOPS: usize = 8;
const MAX_SCANNED_LOOPS: usize = 16;
const SMPL_PREFIX_BYTES: usize = 36 + 24 * MAX_SCANNED_LOOPS;
const MAX_MARK_BYTES: usize = 65_536;
const OGG_TAIL_SCAN_BYTES: u64 = 65_536;

/// Probes an audio file on disk for sample rate, channel count, frame count,
/// and embedded loop points.
///
/// # Errors
/// Returns an [`io::Error`] if the file cannot be opened/read or if its header
/// is unsupported, truncated, or malformed.
pub fn probe_header(path: &Path) -> io::Result<AudioHeader> {
    probe_region(path, 0, None)
}

/// Probes a container at `offset`, with reads bounded to `len` bytes (or EOF).
pub(crate) fn probe_region(path: &Path, offset: u64, len: Option<u64>) -> io::Result<AudioHeader> {
    let file = File::open(path)?;
    let available = file
        .metadata()?
        .len()
        .checked_sub(offset)
        .ok_or_else(|| invalid_data("audio slice starts after EOF"))?;
    let len = len.unwrap_or(available);
    if len > available {
        return Err(invalid_data("audio slice extends past EOF"));
    }
    probe_reader(&mut Region { file, offset }, len)
}

// All parser reads go through read_at, which bounds each read to the region.
struct Region {
    file: File,
    offset: u64,
}

impl Read for Region {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Seek for Region {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let SeekFrom::Start(pos) = pos else {
            return Err(io::ErrorKind::Unsupported.into());
        };
        let absolute = self
            .offset
            .checked_add(pos)
            .ok_or_else(|| invalid_data("audio slice offset overflow"))?;
        self.file
            .seek(SeekFrom::Start(absolute))
            .map(|at| at - self.offset)
    }
}

/// Probes an in-memory audio byte slice for sample rate, channel count, frame
/// count, and embedded loop points.
///
/// # Errors
/// Returns an [`io::Error`] if the bytes are unsupported, truncated, or malformed.
pub fn probe_bytes(bytes: &[u8]) -> io::Result<AudioHeader> {
    let file_len = bytes.len() as u64;
    let mut cursor = Cursor::new(bytes);
    probe_reader(&mut cursor, file_len)
}

fn probe_reader<R: Read + Seek>(reader: &mut R, file_len: u64) -> io::Result<AudioHeader> {
    let mut magic = [0u8; 4];
    read_at(reader, file_len, 0, &mut magic)?;
    match &magic {
        b"RIFF" => probe_wav(reader, file_len, false),
        b"RF64" => probe_wav(reader, file_len, true),
        b"fLaC" => probe_flac(reader, file_len),
        b"FORM" => probe_aiff(reader, file_len),
        b"OggS" => probe_ogg_vorbis(reader, file_len),
        _ => Err(invalid_data("unsupported audio container magic")),
    }
}

fn invalid_data(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn read_at<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
    offset: u64,
    buf: &mut [u8],
) -> io::Result<()> {
    let end = offset
        .checked_add(buf.len() as u64)
        .ok_or_else(|| invalid_data("file offset overflow"))?;
    if end > file_len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "truncated audio header",
        ));
    }
    reader.seek(SeekFrom::Start(offset))?;
    reader.read_exact(buf)
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    let arr: [u8; 2] = slice.try_into().ok()?;
    Some(u16::from_le_bytes(arr))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    let arr: [u8; 4] = slice.try_into().ok()?;
    Some(u32::from_le_bytes(arr))
}

fn le64(bytes: &[u8], at: usize) -> Option<u64> {
    let slice = bytes.get(at..at + 8)?;
    let arr: [u8; 8] = slice.try_into().ok()?;
    Some(u64::from_le_bytes(arr))
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    let arr: [u8; 2] = slice.try_into().ok()?;
    Some(u16::from_be_bytes(arr))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    let arr: [u8; 4] = slice.try_into().ok()?;
    Some(u32::from_be_bytes(arr))
}

/// Parses a RIFF or RF64 WAVE container: `ds64`, `fmt `, `fact`, `data`, and `smpl`.
fn probe_wav<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
    is_rf64: bool,
) -> io::Result<AudioHeader> {
    let mut wave = [0u8; 4];
    read_at(reader, file_len, 8, &mut wave)?;
    if &wave != b"WAVE" {
        return Err(invalid_data("RIFF/RF64 file is not WAVE"));
    }

    let mut pos = 12u64;
    let mut chunks_left = MAX_WAV_PRE_DATA_CHUNKS;

    let mut rf64_data_size: Option<u64> = None;
    let mut rf64_sample_count: Option<u64> = None;
    let mut fmt_info: Option<(u16, u16, u32, u16, u16)> = None;
    let mut fact_samples: Option<u32> = None;
    let mut data_bytes: Option<u64> = None;
    let mut smpl_loc: Option<(u64, u32)> = None;

    let mut hdr = [0u8; 8];
    while chunks_left > 0 && read_at(reader, file_len, pos, &mut hdr).is_ok() {
        chunks_left -= 1;
        let [id0, id1, id2, id3, s0, s1, s2, s3] = hdr;
        let id = [id0, id1, id2, id3];
        let raw_size = u32::from_le_bytes([s0, s1, s2, s3]);
        let body = pos.saturating_add(8);

        let chunk_size: u64 = match &id {
            b"ds64" if is_rf64 => {
                if raw_size >= 28 {
                    let mut ds64 = [0u8; 28];
                    if read_at(reader, file_len, body, &mut ds64).is_ok() {
                        rf64_data_size = le64(&ds64, 8);
                        rf64_sample_count = le64(&ds64, 16);
                    }
                }
                u64::from(raw_size)
            }
            b"fmt " if fmt_info.is_none() => {
                if raw_size < 16 {
                    return Err(invalid_data("WAV fmt chunk too short"));
                }
                let mut fmt = [0u8; 16];
                read_at(reader, file_len, body, &mut fmt)?;
                let mut tag = le16(&fmt, 0).ok_or_else(|| invalid_data("WAV fmt tag"))?;
                if tag == 0xFFFE {
                    if raw_size < 40 {
                        return Err(invalid_data("WAV extensible fmt chunk too short"));
                    }
                    let mut sub = [0u8; 16];
                    read_at(reader, file_len, body + 24, &mut sub)?;
                    if !sub[2..].eq(&[0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]) {
                        return Err(invalid_data("unsupported WAV subformat GUID"));
                    }
                    tag = u16::from_le_bytes([sub[0], sub[1]]);
                }
                let channels = le16(&fmt, 2).ok_or_else(|| invalid_data("WAV channels"))?;
                let sample_rate = le32(&fmt, 4).ok_or_else(|| invalid_data("WAV sample rate"))?;
                let block_align = le16(&fmt, 12).ok_or_else(|| invalid_data("WAV block align"))?;
                let bits_per_sample =
                    le16(&fmt, 14).ok_or_else(|| invalid_data("WAV bits per sample"))?;
                fmt_info = Some((tag, channels, sample_rate, block_align, bits_per_sample));
                u64::from(raw_size)
            }
            b"fact" if fact_samples.is_none() => {
                if raw_size >= 4 {
                    let mut fact = [0u8; 4];
                    if read_at(reader, file_len, body, &mut fact).is_ok() {
                        fact_samples = Some(u32::from_le_bytes(fact));
                    }
                }
                u64::from(raw_size)
            }
            b"data" if data_bytes.is_none() => {
                let declared = if is_rf64 && raw_size == u32::MAX {
                    rf64_data_size.unwrap_or(0)
                } else {
                    u64::from(raw_size)
                };
                let avail = file_len.saturating_sub(body);
                data_bytes = Some(declared.min(avail));
                chunks_left = MAX_METADATA_CHUNKS;
                declared
            }
            b"smpl"
                if smpl_loc.is_none() && u64::from(raw_size) <= file_len.saturating_sub(body) =>
            {
                smpl_loc = Some((body, raw_size));
                u64::from(raw_size)
            }
            _ => u64::from(raw_size),
        };

        let padded = chunk_size.saturating_add(chunk_size & 1);
        let Some(next) = body.checked_add(padded) else {
            break;
        };
        if next <= pos || next > file_len {
            break;
        }
        pos = next;
    }

    let (tag, channels, sample_rate, block_align, bits_per_sample) =
        fmt_info.ok_or_else(|| invalid_data("WAV missing fmt chunk"))?;
    if channels == 0 || sample_rate == 0 {
        return Err(invalid_data("WAV has zero channels or sample rate"));
    }

    // The playback reader supports PCM and IEEE float, both frame-aligned.
    if !matches!(tag, 1 | 3) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported WAV codec",
        ));
    }

    let frame_bytes = if block_align > 0 {
        u64::from(block_align)
    } else {
        u64::from(channels).saturating_mul(u64::from(bits_per_sample.div_ceil(8)))
    };

    let frames = if let Some(bytes) = data_bytes {
        if let Some(frames) = bytes.checked_div(frame_bytes) {
            frames
        } else if let Some(count) = rf64_sample_count {
            count
        } else if let Some(count) = fact_samples {
            u64::from(count)
        } else {
            return Err(invalid_data("WAV has zero block align"));
        }
    } else if let Some(count) = rf64_sample_count {
        count
    } else if let Some(count) = fact_samples {
        u64::from(count)
    } else {
        return Err(invalid_data("WAV missing data chunk"));
    };

    let loops = match smpl_loc {
        Some((at, len)) => read_smpl_loops(reader, file_len, at, len, frames),
        None => Vec::new(),
    };

    Ok(AudioHeader {
        sample_rate,
        channels,
        frames,
        loops,
    })
}

/// Reads and parses a WAV `smpl` chunk payload (bounded to [`SMPL_PREFIX_BYTES`]).
fn read_smpl_loops<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
    at: u64,
    len: u32,
    frames: u64,
) -> Vec<AudioLoop> {
    let take = (len as usize).min(SMPL_PREFIX_BYTES);
    let mut buf = [0u8; SMPL_PREFIX_BYTES];
    let Some(slice) = buf.get_mut(..take) else {
        return Vec::new();
    };
    if read_at(reader, file_len, at, slice).is_err() {
        return Vec::new();
    }
    parse_smpl_loops(slice, frames)
}

/// Parses RIFF `smpl` loop records (offset 36, 24 bytes each).
///
/// Loop type 0 is forward, 1 is ping-pong; type 2 (reverse) and unknown types
/// are skipped. `dwEnd` in RIFF `smpl` is inclusive, so we add 1 and clamp to
/// `frames`. A `dwPlayCount` of 0 repeats infinitely; 1 plays once (no loop);
/// `n > 1` repeats `n - 1` times.
fn parse_smpl_loops(body: &[u8], frames: u64) -> Vec<AudioLoop> {
    let count = le32(body, 28).unwrap_or(0) as usize;
    (0..count.min(MAX_SCANNED_LOOPS))
        .filter_map(|i| {
            let at = 36 + 24 * i;
            let loop_type = le32(body, at + 4)?;
            let start = u64::from(le32(body, at + 8)?);
            let end = u64::from(le32(body, at + 12)?)
                .saturating_add(1)
                .min(frames);
            let repeats = match le32(body, at + 20)? {
                0 => 0,
                1 => return None,
                plays => plays - 1,
            };
            (loop_type <= 1 && start < end).then_some(AudioLoop {
                start,
                end,
                count: repeats,
                ping_pong: loop_type == 1,
            })
        })
        .take(MAX_INLINE_LOOPS)
        .collect()
}

/// Parses a FLAC container: `STREAMINFO` plus optional `APPLICATION` (`riff`) `smpl` loops.
fn probe_flac<R: Read + Seek>(reader: &mut R, file_len: u64) -> io::Result<AudioHeader> {
    let mut pos = 4u64;
    let mut stream_info: Option<(u32, u16, u64)> = None;
    let mut smpl_loc: Option<(u64, u32)> = None;

    let mut hdr = [0u8; 4];
    for _ in 0..MAX_METADATA_CHUNKS {
        if read_at(reader, file_len, pos, &mut hdr).is_err() {
            break;
        }
        let [kind, l0, l1, l2] = hdr;
        let is_last = (kind & 0x80) != 0;
        let block_type = kind & 0x7F;
        let block_len = u32::from_be_bytes([0, l0, l1, l2]);
        let body = pos.saturating_add(4);
        let Some(next) = body
            .checked_add(u64::from(block_len))
            .filter(|&end| end <= file_len)
        else {
            break;
        };

        if block_type == 0 && block_len >= 34 && stream_info.is_none() {
            let mut packed = [0u8; 8];
            read_at(reader, file_len, body + 10, &mut packed)?;
            let word = u64::from_be_bytes(packed);
            let sample_rate = (word >> 44) as u32;
            let channels = (((word >> 41) & 0x7) + 1) as u16;
            let total_samples = word & ((1u64 << 36) - 1);
            stream_info = Some((sample_rate, channels, total_samples));
        } else if block_type == 2 && block_len >= 12 && smpl_loc.is_none() {
            let mut riff = [0u8; 12];
            if read_at(reader, file_len, body, &mut riff).is_ok() && riff.starts_with(b"riffsmpl") {
                let declared = le32(&riff, 8).unwrap_or(0);
                let avail = block_len.saturating_sub(12);
                smpl_loc = Some((body + 12, avail.min(declared)));
            }
        }

        if is_last {
            break;
        }
        pos = next;
    }

    let (sample_rate, channels, frames) =
        stream_info.ok_or_else(|| invalid_data("FLAC missing STREAMINFO block"))?;
    if frames == 0 {
        return Err(invalid_data(
            "FLAC sample length is unknown (STREAMINFO sample count is zero)",
        ));
    }
    if sample_rate == 0 || channels == 0 {
        return Err(invalid_data("FLAC has zero sample rate or channels"));
    }

    let loops = match smpl_loc {
        Some((at, len)) => read_smpl_loops(reader, file_len, at, len, frames),
        None => Vec::new(),
    };

    Ok(AudioHeader {
        sample_rate,
        channels,
        frames,
        loops,
    })
}

/// Parses an AIFF or AIFC container: `COMM` (with 80-bit IEEE 754 extended sample rate)
/// plus optional `MARK` and `INST` sustain loops.
fn probe_aiff<R: Read + Seek>(reader: &mut R, file_len: u64) -> io::Result<AudioHeader> {
    let mut form_type = [0u8; 4];
    read_at(reader, file_len, 8, &mut form_type)?;
    if &form_type != b"AIFF" && &form_type != b"AIFC" {
        return Err(invalid_data("FORM container is not AIFF/AIFC"));
    }

    let aifc = &form_type == b"AIFC";
    let mut form_size = [0u8; 4];
    read_at(reader, file_len, 4, &mut form_size)?;
    let file_len = file_len.min(8 + u64::from(u32::from_be_bytes(form_size)));

    let mut pos = 12u64;
    let mut chunks_left = MAX_WAV_PRE_DATA_CHUNKS;
    let mut comm = None;
    let mut sound_bytes = None;
    let mut markers: Vec<(u16, u64)> = Vec::new();
    let mut sustain_loop: Option<(u16, u16, u16)> = None;

    let mut hdr = [0u8; 8];
    while chunks_left > 0 && read_at(reader, file_len, pos, &mut hdr).is_ok() {
        chunks_left -= 1;
        let [id0, id1, id2, id3, s0, s1, s2, s3] = hdr;
        let id = [id0, id1, id2, id3];
        let raw_size = u32::from_be_bytes([s0, s1, s2, s3]);
        let body = pos.saturating_add(8);

        match &id {
            b"COMM" if comm.is_none() => {
                if raw_size < 18 {
                    return Err(invalid_data("AIFF COMM chunk too short"));
                }
                if aifc {
                    if raw_size < 22 {
                        return Err(invalid_data("AIFC COMM chunk lacks a codec"));
                    }
                    let mut codec = [0; 4];
                    read_at(reader, file_len, body + 18, &mut codec)?;
                    if !matches!(&codec, b"NONE" | b"twos" | b"sowt" | b"fl32" | b"FL32") {
                        return Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "unsupported AIFC codec",
                        ));
                    }
                }
                let mut buf = [0u8; 18];
                read_at(reader, file_len, body, &mut buf)?;
                let channels = be16(&buf, 0).ok_or_else(|| invalid_data("AIFF channels"))?;
                let frames = u64::from(be32(&buf, 2).ok_or_else(|| invalid_data("AIFF frames"))?);
                let rate_bytes: [u8; 10] = buf
                    .get(8..18)
                    .and_then(|s| s.try_into().ok())
                    .ok_or_else(|| invalid_data("AIFF sample rate bytes"))?;
                let sample_rate = parse_ieee_extended(rate_bytes)?;
                let bits = be16(&buf, 6).ok_or_else(|| invalid_data("AIFF sample size"))?;
                let frame_bytes = u64::from(channels) * u64::from(bits.div_ceil(8));
                comm = Some((channels, sample_rate, frames, frame_bytes));
            }
            b"MARK" if markers.is_empty() && raw_size >= 2 => {
                let take = (raw_size as usize).min(MAX_MARK_BYTES);
                if body.saturating_add(take as u64) <= file_len {
                    let mut buf = vec![0u8; take];
                    if read_at(reader, file_len, body, &mut buf).is_ok() {
                        markers = parse_aiff_markers(&buf);
                    }
                }
            }
            b"INST" if sustain_loop.is_none() && raw_size >= 20 => {
                let mut buf = [0u8; 20];
                if read_at(reader, file_len, body, &mut buf).is_ok()
                    && let (Some(play_mode), Some(begin_id), Some(end_id)) =
                        (be16(&buf, 8), be16(&buf, 10), be16(&buf, 12))
                    && (play_mode == 1 || play_mode == 2)
                {
                    sustain_loop = Some((play_mode, begin_id, end_id));
                }
            }
            b"SSND" if sound_bytes.is_none() => {
                let mut prefix = [0u8; 8];
                if raw_size < 8 {
                    return Err(invalid_data("AIFF SSND chunk too short"));
                }
                read_at(reader, file_len, body, &mut prefix)?;
                let offset =
                    u64::from(be32(&prefix, 0).ok_or_else(|| invalid_data("AIFF sound offset"))?);
                let payload = u64::from(raw_size).min(file_len - body) - 8;
                sound_bytes = Some(
                    payload
                        .checked_sub(offset)
                        .ok_or_else(|| invalid_data("AIFF sound offset exceeds SSND payload"))?,
                );
                chunks_left = chunks_left.min(MAX_METADATA_CHUNKS);
            }
            _ => {}
        }

        let size_u64 = u64::from(raw_size);
        let padded = size_u64.saturating_add(size_u64 & 1);
        let Some(next) = body.checked_add(padded) else {
            break;
        };
        if next <= pos || next > file_len {
            break;
        }
        pos = next;
    }

    let (channels, sample_rate, frames, frame_bytes) =
        comm.ok_or_else(|| invalid_data("AIFF missing COMM chunk"))?;
    if channels == 0 || sample_rate == 0 {
        return Err(invalid_data("AIFF has zero channels or sample rate"));
    }

    if frame_bytes == 0 || frames * frame_bytes > sound_bytes.unwrap_or(0) {
        return Err(invalid_data(
            "AIFF declared frames exceed available sound data",
        ));
    }

    let mut loops = Vec::new();
    if let Some((play_mode, begin_id, end_id)) = sustain_loop {
        let start = markers
            .iter()
            .find_map(|&(id, pos)| (id == begin_id).then_some(pos));
        let end = markers
            .iter()
            .find_map(|&(id, pos)| (id == end_id).then_some(pos));
        if let (Some(start), Some(end)) = (start, end) {
            let clamped_end = end.min(frames);
            if start < clamped_end {
                loops.push(AudioLoop {
                    start,
                    end: clamped_end,
                    count: 0,
                    ping_pong: play_mode == 2,
                });
            }
        }
    }

    Ok(AudioHeader {
        sample_rate,
        channels,
        frames,
        loops,
    })
}

/// Decodes an 80-bit big-endian IEEE 754 extended float sample rate from AIFF `COMM`.
fn parse_ieee_extended(bytes: [u8; 10]) -> io::Result<u32> {
    let [b0, b1, m0, m1, m2, m3, m4, m5, m6, m7] = bytes;
    let negative = (b0 & 0x80) != 0;
    let exponent = (u16::from(b0 & 0x7F) << 8) | u16::from(b1);
    let mantissa = u64::from_be_bytes([m0, m1, m2, m3, m4, m5, m6, m7]);
    if negative || exponent == 0x7FFF || (exponent == 0 && mantissa == 0) {
        return Err(invalid_data("invalid AIFF extended sample rate"));
    }
    let value = (mantissa as f64) * 2.0_f64.powi(i32::from(exponent) - 16383 - 63);
    if !value.is_finite() || value < 1.0 || value > f64::from(u32::MAX) {
        return Err(invalid_data("out-of-range AIFF sample rate"));
    }
    Ok(value.round() as u32)
}

/// Parses an AIFF `MARK` chunk into `(marker_id, frame_position)` pairs.
fn parse_aiff_markers(buf: &[u8]) -> Vec<(u16, u64)> {
    let Some(num_markers) = be16(buf, 0) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(usize::from(num_markers).min(64));
    let mut at = 2usize;
    for _ in 0..num_markers {
        let Some(id) = be16(buf, at) else {
            break;
        };
        let Some(position) = be32(buf, at + 2) else {
            break;
        };
        let Some(&name_len) = buf.get(at + 6) else {
            break;
        };
        let name_len = usize::from(name_len);
        // Pascal string: 1 length byte + name_len bytes + 1 pad byte if total (1 + name_len) is odd.
        let pad = usize::from(name_len.is_multiple_of(2));
        let Some(next) = at
            .checked_add(7)
            .and_then(|v| v.checked_add(name_len))
            .and_then(|v| v.checked_add(pad))
        else {
            break;
        };
        if next > buf.len() {
            break;
        }
        out.push((id, u64::from(position)));
        at = next;
    }
    out
}

/// Parses an OGG/Vorbis container: identification header (`\x01vorbis`) and
/// bounded tail scan for the last Ogg page's granule position.
fn probe_ogg_vorbis<R: Read + Seek>(reader: &mut R, file_len: u64) -> io::Result<AudioHeader> {
    let mut page_hdr = [0u8; 27];
    read_at(reader, file_len, 0, &mut page_hdr)?;
    if page_hdr.get(4).copied() != Some(0) {
        return Err(invalid_data("unsupported Ogg page version"));
    }
    let serial = le32(&page_hdr, 14).ok_or_else(|| invalid_data("Ogg serial"))?;
    let n_segments = usize::from(page_hdr.get(26).copied().unwrap_or(0));
    if n_segments == 0 {
        return Err(invalid_data("Ogg BOS page has no segments"));
    }

    let mut seg_table = [0u8; 255];
    let seg_slice = seg_table
        .get_mut(..n_segments)
        .ok_or_else(|| invalid_data("Ogg segment table overflow"))?;
    read_at(reader, file_len, 27, seg_slice)?;
    let payload_len: usize = seg_slice.iter().map(|&b| usize::from(b)).sum();
    if payload_len < 30 {
        return Err(invalid_data("Ogg Vorbis identification header too short"));
    }

    let mut id_hdr = [0u8; 30];
    read_at(reader, file_len, 27 + n_segments as u64, &mut id_hdr)?;
    if !id_hdr.starts_with(b"\x01vorbis") {
        return Err(invalid_data("Ogg stream is not Vorbis"));
    }
    if le32(&id_hdr, 7) != Some(0) {
        return Err(invalid_data("unsupported Vorbis version"));
    }
    let channels = u16::from(id_hdr.get(11).copied().unwrap_or(0));
    let sample_rate = le32(&id_hdr, 12).ok_or_else(|| invalid_data("Vorbis sample rate"))?;
    if channels == 0 || sample_rate == 0 {
        return Err(invalid_data("Vorbis has zero channels or sample rate"));
    }

    // Bounded tail scan for the final matching OggS page's granule position.
    let tail_start = file_len.saturating_sub(OGG_TAIL_SCAN_BYTES);
    let tail_len = (file_len - tail_start) as usize;
    let mut tail = vec![0u8; tail_len];
    read_at(reader, file_len, tail_start, &mut tail)?;

    let mut frames = 0u64;
    if tail_len >= 27 {
        for pos in (0..=tail_len - 27).rev() {
            let Some(hdr) = tail.get(pos..pos + 27) else {
                continue;
            };
            if !hdr.starts_with(b"OggS") || hdr.get(4).copied() != Some(0) {
                continue;
            }
            if le32(hdr, 14) != Some(serial) {
                continue;
            }
            let Some(raw_granule) = le64(hdr, 6) else {
                continue;
            };
            let granule = raw_granule as i64;
            if granule >= 0 {
                frames = granule as u64;
                break;
            }
        }
    }

    Ok(AudioHeader {
        sample_rate,
        channels,
        frames,
        loops: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_wav(
        channels: u16,
        sample_rate: u32,
        frames: u32,
        loops: &[(u32, u32, u32, u32)],
    ) -> Vec<u8> {
        let block_align = channels * 2;
        let data_len = frames * u32::from(block_align);
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(channels.to_le_bytes());
        wav.extend(sample_rate.to_le_bytes());
        wav.extend((sample_rate * u32::from(block_align)).to_le_bytes());
        wav.extend(block_align.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        wav.resize(wav.len() + data_len as usize, 0);

        if !loops.is_empty() {
            let mut body = vec![0u8; 36];
            body[28..32].copy_from_slice(&(loops.len() as u32).to_le_bytes());
            for (i, &(loop_type, start, end, play_count)) in loops.iter().enumerate() {
                for word in [i as u32, loop_type, start, end, 0, play_count] {
                    body.extend(word.to_le_bytes());
                }
            }
            wav.extend(b"smpl");
            wav.extend((body.len() as u32).to_le_bytes());
            wav.extend(body);
        }

        let riff_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
        wav
    }

    fn build_flac(channels: u16, sample_rate: u32, frames: u64, with_smpl: bool) -> Vec<u8> {
        let packed: u64 = (u64::from(sample_rate) << 44)
            | (u64::from(channels - 1) << 41)
            | (15u64 << 36)
            | (frames & ((1u64 << 36) - 1));
        let mut flac = b"fLaC".to_vec();
        flac.push(if with_smpl { 0x00 } else { 0x80 }); // STREAMINFO
        flac.extend([0, 0, 34]);
        flac.extend([0u8; 10]);
        flac.extend(packed.to_be_bytes());
        flac.extend([0u8; 16]);

        if with_smpl {
            let mut smpl = vec![0u8; 36];
            smpl[28..32].copy_from_slice(&1u32.to_le_bytes());
            for word in [0u32, 0, 100, 499, 0, 0] {
                smpl.extend(word.to_le_bytes());
            }
            let mut app = b"riffsmpl".to_vec();
            app.extend((smpl.len() as u32).to_le_bytes());
            app.extend(smpl);
            flac.push(0x82); // last block, APPLICATION
            let len_bytes = (app.len() as u32).to_be_bytes();
            flac.extend(&len_bytes[1..]);
            flac.extend(app);
        }
        flac
    }

    fn encode_ieee_extended_44100() -> [u8; 10] {
        // 44100 = 0xAC44, exponent = 16383 + 15 = 0x400E, mantissa = 0xAC44 << 48
        [0x40, 0x0E, 0xAC, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    }

    fn build_aiff(channels: u16, frames: u32, with_loop: bool) -> Vec<u8> {
        let mut body = b"AIFF".to_vec();
        // COMM chunk
        body.extend(b"COMM");
        body.extend(18u32.to_be_bytes());
        body.extend(channels.to_be_bytes());
        body.extend(frames.to_be_bytes());
        body.extend(16u16.to_be_bytes());
        body.extend(encode_ieee_extended_44100());

        if with_loop {
            // MARK chunk with 2 markers (ids 1 and 2)
            let mut mark = Vec::new();
            mark.extend(2u16.to_be_bytes());
            // marker 1 at frame 200, name "start" (len 5, odd -> 0 pad)
            mark.extend(1u16.to_be_bytes());
            mark.extend(200u32.to_be_bytes());
            mark.push(5);
            mark.extend(b"start");
            // marker 2 at frame 800, name "end!" (len 4, even -> 1 pad)
            mark.extend(2u16.to_be_bytes());
            mark.extend(800u32.to_be_bytes());
            mark.push(4);
            mark.extend(b"end!");
            mark.push(0);

            body.extend(b"MARK");
            body.extend((mark.len() as u32).to_be_bytes());
            body.extend(&mark);
            if mark.len() % 2 == 1 {
                body.push(0);
            }

            // INST chunk (20 bytes): ping-pong sustain loop from marker 1 to marker 2
            let mut inst = [0u8; 20];
            inst[8..10].copy_from_slice(&2u16.to_be_bytes()); // ping-pong
            inst[10..12].copy_from_slice(&1u16.to_be_bytes());
            inst[12..14].copy_from_slice(&2u16.to_be_bytes());
            body.extend(b"INST");
            body.extend(20u32.to_be_bytes());
            body.extend(inst);
        }

        let data_bytes = frames * u32::from(channels) * 2;
        body.extend(b"SSND");
        body.extend((8 + data_bytes).to_be_bytes());
        body.resize(body.len() + 8 + data_bytes as usize, 0);

        let mut aiff = b"FORM".to_vec();
        aiff.extend((body.len() as u32).to_be_bytes());
        aiff.extend(body);
        aiff
    }

    fn build_ogg_vorbis(channels: u8, sample_rate: u32, total_frames: u64) -> Vec<u8> {
        let serial = 0x1234_5678u32;
        let mut ogg = Vec::new();
        // Page 0: BOS with Vorbis identification header
        ogg.extend(b"OggS");
        ogg.push(0); // version
        ogg.push(0x02); // BOS
        ogg.extend(0i64.to_le_bytes()); // granule
        ogg.extend(serial.to_le_bytes());
        ogg.extend(0u32.to_le_bytes()); // seq
        ogg.extend(0u32.to_le_bytes()); // crc
        ogg.push(1); // 1 segment
        ogg.push(30); // 30-byte payload
        // Vorbis ID header
        ogg.extend(b"\x01vorbis");
        ogg.extend(0u32.to_le_bytes()); // vorbis version 0
        ogg.push(channels);
        ogg.extend(sample_rate.to_le_bytes());
        ogg.extend([0u8; 14]); // bitrates + blocksize + framing

        // Page 1: EOS with granule position = total_frames
        ogg.extend(b"OggS");
        ogg.push(0);
        ogg.push(0x04); // EOS
        ogg.extend((total_frames as i64).to_le_bytes());
        ogg.extend(serial.to_le_bytes());
        ogg.extend(1u32.to_le_bytes());
        ogg.extend(0u32.to_le_bytes());
        ogg.push(1);
        ogg.push(4);
        ogg.extend([0u8; 4]);
        ogg
    }

    #[test]
    fn sample_pool_probes_and_caches_the_registered_region() {
        use crate::{Notes, builder::SamplePool};

        let first = build_wav(1, 44_100, 100, &[]);
        let second = build_wav(2, 48_000, 900, &[]);
        let file = std::env::temp_dir().join(format!("kontra-regions-{}.wav", std::process::id()));
        std::fs::write(&file, [&first[..], &second[..]].concat()).unwrap();

        let mut pool = SamplePool::new();
        let mut notes = Notes::default();
        let (whole, header) = pool.add("whole", &file, &mut notes);
        assert_eq!(header.unwrap().frames, 100);
        let offset = first.len() as u64;
        let len = Some(second.len() as u64);
        let (slice, header) = pool.add_slice("second", &file, offset, len, &mut notes);
        assert_eq!(header.unwrap().frames, 900);
        assert_ne!(slice, whole);
        assert_eq!(pool.samples()[slice].frames, 900);
        assert_eq!(pool.samples()[slice].rate, 48_000);
        assert_eq!(
            pool.add_slice("again", &file, offset, len, &mut notes).0,
            slice
        );
        assert!(
            pool.add_slice("truncated", &file, offset, Some(12), &mut notes)
                .1
                .is_none()
        );
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn rejects_block_compressed_wav_even_with_a_fact_count() {
        let mut wav = build_wav(1, 44_100, 16, &[]);
        wav[20..22].copy_from_slice(&0x11u16.to_le_bytes()); // IMA ADPCM
        wav[32..34].copy_from_slice(&16u16.to_le_bytes());
        wav.splice(36..36, b"fact\x04\0\0\0\xE8\x03\0\0".iter().copied());
        let size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(
            probe_bytes(&wav).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn flac_with_unknown_length_reports_an_error() {
        let err = probe_bytes(&build_flac(1, 44_100, 0, false)).unwrap_err();
        assert!(err.to_string().contains("FLAC sample length is unknown"));
    }

    #[test]
    fn probes_wav_with_smpl_loops() {
        let wav = build_wav(2, 48_000, 2_000, &[(0, 100, 499, 0), (1, 600, 999, 3)]);
        let hdr = probe_bytes(&wav).unwrap();
        assert_eq!(hdr.sample_rate, 48_000);
        assert_eq!(hdr.channels, 2);
        assert_eq!(hdr.frames, 2_000);
        assert_eq!(
            hdr.loops,
            vec![
                AudioLoop {
                    start: 100,
                    end: 500,
                    count: 0,
                    ping_pong: false,
                },
                AudioLoop {
                    start: 600,
                    end: 1000,
                    count: 2,
                    ping_pong: true,
                },
            ]
        );
    }

    #[test]
    fn probes_rf64_wav() {
        let mut wav = b"RF64\xFF\xFF\xFF\xFFWAVE".to_vec();
        // ds64 chunk: riff_size=0, data_size=4000, sample_count=1000
        wav.extend(b"ds64");
        wav.extend(28u32.to_le_bytes());
        wav.extend(0u64.to_le_bytes());
        wav.extend(4000u64.to_le_bytes());
        wav.extend(1000u64.to_le_bytes());
        wav.extend(0u32.to_le_bytes());
        // fmt chunk: stereo 44100 16-bit
        wav.extend(b"fmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(44_100u32.to_le_bytes());
        wav.extend(176_400u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        // data chunk with 0xFFFFFFFF size marker
        wav.extend(b"data");
        wav.extend(u32::MAX.to_le_bytes());
        wav.resize(wav.len() + 4000, 0);

        let hdr = probe_bytes(&wav).unwrap();
        assert_eq!(hdr.sample_rate, 44_100);
        assert_eq!(hdr.channels, 2);
        assert_eq!(hdr.frames, 1000);
    }

    #[test]
    fn probes_flac_and_embedded_riff_smpl() {
        let flac = build_flac(2, 96_000, 48_000, true);
        let hdr = probe_bytes(&flac).unwrap();
        assert_eq!(hdr.sample_rate, 96_000);
        assert_eq!(hdr.channels, 2);
        assert_eq!(hdr.frames, 48_000);
        assert_eq!(
            hdr.loops,
            vec![AudioLoop {
                start: 100,
                end: 500,
                count: 0,
                ping_pong: false,
            }]
        );
    }

    #[test]
    fn rejects_aiff_frames_not_backed_by_sound_data() {
        let mut aiff = build_aiff(2, 4, false);
        let ssnd_offset = 12 + 8 + 18 + 8;
        aiff[ssnd_offset..ssnd_offset + 4].copy_from_slice(&4u32.to_be_bytes());
        // The leading four bytes are offset padding, leaving exactly three frames.
        let err = probe_bytes(&aiff).unwrap_err();
        assert!(err.to_string().contains("declared frames exceed"));
        aiff[22..26].copy_from_slice(&3u32.to_be_bytes());
        assert_eq!(probe_bytes(&aiff).unwrap().frames, 3);
        let err = probe_bytes(&aiff[..aiff.len() - 1]).unwrap_err();
        assert!(err.to_string().contains("declared frames exceed"));
    }

    #[test]
    fn probes_aiff_with_ieee_extended_rate_and_inst_mark_loop() {
        let aiff = build_aiff(2, 4_096, true);
        let hdr = probe_bytes(&aiff).unwrap();
        assert_eq!(hdr.sample_rate, 44_100);
        assert_eq!(hdr.channels, 2);
        assert_eq!(hdr.frames, 4_096);
        assert_eq!(
            hdr.loops,
            vec![AudioLoop {
                start: 200,
                end: 800,
                count: 0,
                ping_pong: true,
            }]
        );
    }

    #[test]
    fn probes_ogg_vorbis_and_tail_granule() {
        let ogg = build_ogg_vorbis(2, 48_000, 123_456);
        let hdr = probe_bytes(&ogg).unwrap();
        assert_eq!(hdr.sample_rate, 48_000);
        assert_eq!(hdr.channels, 2);
        assert_eq!(hdr.frames, 123_456);
        assert!(hdr.loops.is_empty());
    }

    #[test]
    fn rejects_truncated_and_invalid_headers() {
        assert!(probe_bytes(b"").is_err());
        assert!(probe_bytes(b"NOPE12345678").is_err());
        assert!(probe_bytes(b"RIFF\x04\x00\x00\x00AVI ").is_err());
        assert!(probe_bytes(b"fLaC\x80\x00\x00\x04\x00\x00\x00\x00").is_err());
        assert!(probe_bytes(b"FORM\x00\x00\x00\x04AIFF").is_err());
        assert!(probe_bytes(b"OggS\x00\x02").is_err());
    }
}
