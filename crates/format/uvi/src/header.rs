//! What a sample file's own header declares: channels, rate, frames and
//! loops for PCM WAV and FLAC. The engine sniffs the codec from the same bytes; the
//! instrument model needs the length up front to place zones.

/// The declared layout of one sample file.
#[derive(Debug, PartialEq)]
pub(crate) struct Format {
    pub channels: u8,
    pub rate: u32,
    pub frames: u64,
    /// Its `smpl` chunk's loops, which a player plays unless told otherwise.
    pub loops: Vec<kontra_ir::Loop>,
}

/// Reads only the headers and supported metadata of a bank member. UVI
/// members hold FLAC behind `.wav` names, so the magic decides.
pub(crate) fn probe(
    size: u64,
    mut read_at: impl FnMut(u64, &mut [u8]) -> Option<()>,
) -> Option<Format> {
    let mut read = |at: u64, bytes: &mut [u8]| {
        if at.checked_add(bytes.len() as u64)? > size {
            return None;
        }
        read_at(at, bytes)
    };
    let mut magic = [0; 4];
    read(0, &mut magic)?;
    match &magic {
        b"fLaC" => flac(size, read),
        b"RIFF" => wav(size, read),
        _ => None,
    }
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

// Limit positioned reads by header count, so large payloads can still be
// skipped. Keep the format and any loops already found when the scan ends.
const METADATA_HEADERS: usize = 256;
// Preserve coverage for WAV layout headers in the old 64 KiB probe even
// if that prefix is packed with minimum-size (eight-byte) chunk headers.
const WAV_LAYOUT_HEADERS: usize = 64 * 1024 / 8;

/// The STREAMINFO block: its rate, channels and total frames packed after
/// the 10 bytes of block sizes (RFC 9639 §8.2). Zero frames means unknown.
fn flac(size: u64, mut read: impl FnMut(u64, &mut [u8]) -> Option<()>) -> Option<Format> {
    let mut packed = [0; 8];
    read(18, &mut packed)?;
    let packed = u64::from_be_bytes(packed);
    let rate = (packed >> 44) as u32;
    let channels = ((packed >> 41) & 7) as u8 + 1;
    let frames = packed & ((1 << 36) - 1);
    if rate == 0 || frames == 0 {
        return None;
    }
    // Each foreign WAV chunk occupies an APPLICATION block (type 2)
    // with id `riff` (RFC 9639 §8.4; flac's --keep-foreign-metadata).
    let mut loops = Vec::new();
    let mut at = 4u64;
    let mut block = [0; 4];
    for _ in 0..METADATA_HEADERS {
        if read(at, &mut block).is_none() {
            break;
        }
        let kind = block[0];
        let len = u32::from_be_bytes([0, block[1], block[2], block[3]]);
        let body = at + 4;
        let Some(next) = body.checked_add(u64::from(len)).filter(|&end| end <= size) else {
            break;
        };
        if kind & 0x7f == 2 && len >= 12 {
            let mut riff = [0; 12];
            if read(body, &mut riff).is_some() && riff.starts_with(b"riffsmpl") {
                let len = (len - 12).min(le32(&riff, 8).unwrap_or(0));
                if let Some(found) = read_smpl(body + 12, len, frames, &mut read) {
                    loops = found;
                }
            }
        }
        if kind & 0x80 != 0 {
            break;
        }
        at = next;
    }
    Some(Format {
        channels,
        rate,
        frames,
        loops,
    })
}

// Keep file loops within kontra_voice::Loops' inline capacity. The engine
// clones a zone's region when playback starts on the audio thread.
const INLINE_LOOPS: usize = 8;
const SCANNED_LOOPS: usize = 16;
const SMPL_PREFIX: usize = 36 + 24 * SCANNED_LOOPS;

/// Read loop records only, leaving sampler-specific payloads on disk.
fn read_smpl(
    at: u64,
    len: u32,
    frames: u64,
    read: &mut impl FnMut(u64, &mut [u8]) -> Option<()>,
) -> Option<Vec<kontra_ir::Loop>> {
    let mut body = [0; SMPL_PREFIX];
    let body = body.get_mut(..(len as usize).min(SMPL_PREFIX))?;
    read(at, body)?;
    Some(smpl(body, frames))
}

/// A `smpl` chunk's body: its sample loops (offset 36, 24 bytes each: id,
/// type, first and last frame, fraction, play count). Type 0 loops forward,
/// 1 alternates; 2 (backward) is not read. A count of 0 repeats forever.
fn smpl(body: &[u8], frames: u64) -> Vec<kontra_ir::Loop> {
    let count = le32(body, 28).unwrap_or(0) as usize;
    (0..count.min(SCANNED_LOOPS))
        .filter_map(|k| {
            let at = 36 + 24 * k;
            let kind = le32(body, at + 4)?;
            let start = u64::from(le32(body, at + 8)?);
            let end = (u64::from(le32(body, at + 12)?) + 1).min(frames);
            let count = le32(body, at + 20)?;
            if count == 1 {
                return None; // One traversal needs no repeat.
            }
            (kind <= 1 && start < end).then_some(kontra_ir::Loop {
                start,
                end,
                crossfade: 0,
                count,
                alternating: kind == 1,
                until_release: false,
                tune_ratio: 1.0,
            })
        })
        .take(INLINE_LOOPS)
        .collect()
}

/// The `fmt ` chunk, then the `data` chunk's length in whole frames, clamped to
/// the bytes the member holds (streamed files may declare more). Only PCM and
/// float are read (tag 1 or 3, directly or as the extensible subformat), as
/// the engine's reader.
fn wav(size: u64, mut read: impl FnMut(u64, &mut [u8]) -> Option<()>) -> Option<Format> {
    let mut wave = [0; 4];
    read(8, &mut wave)?;
    if &wave != b"WAVE" {
        return None;
    }
    let mut at = 12u64;
    let mut fmt = None;
    let mut data = None;
    let mut sampler = None;
    let mut chunk = [0; 8];
    let mut chunks_left = WAV_LAYOUT_HEADERS;
    while chunks_left > 0 && read(at, &mut chunk).is_some() {
        chunks_left -= 1;
        let len = le32(&chunk, 4)?;
        let body = at + 8;
        match &chunk[..4] {
            b"fmt " if data.is_none() => {
                let mut layout = [0; 14];
                if len < layout.len() as u32 {
                    return None;
                }
                read(body, &mut layout)?;
                let tag = match le16(&layout, 0)? {
                    0xFFFE if len >= 26 => {
                        let mut subformat = [0; 2];
                        read(body + 24, &mut subformat)?;
                        u16::from_le_bytes(subformat)
                    }
                    tag => tag,
                };
                if tag != 1 && tag != 3 {
                    return None;
                }
                let channels = le16(&layout, 2)?;
                let rate = le32(&layout, 4)?;
                let align = le16(&layout, 12)?;
                fmt = Some((channels, rate, align));
            }
            b"data" if data.is_none() => {
                let (channels, rate, align) = fmt?;
                let bytes = u64::from(len).min(size - body);
                if align == 0 || channels == 0 || channels > 255 {
                    return None;
                }
                data = Some((channels as u8, rate, bytes / u64::from(align)));
                chunks_left = METADATA_HEADERS;
            }
            b"smpl" if u64::from(len) <= size - body => sampler = Some((body, len)),
            _ => {}
        }
        let Some(next) = body.checked_add(u64::from(len) + u64::from(len & 1)) else {
            break;
        };
        at = next;
    }
    let (channels, rate, frames) = data?;
    let loops = sampler
        .and_then(|(at, len)| read_smpl(at, len, frames, &mut read))
        .unwrap_or_default();
    Some(Format {
        channels,
        rate,
        frames,
        loops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_bytes(bytes: &[u8], size: u64) -> Option<Format> {
        probe(size, |at, out| {
            let at = usize::try_from(at).ok()?;
            out.copy_from_slice(bytes.get(at..at + out.len())?);
            Some(())
        })
    }

    fn wav_header(data_len: u32) -> Vec<u8> {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(2u16.to_le_bytes()); // stereo
        wav.extend(48_000u32.to_le_bytes());
        wav.extend(192_000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes()); // block align
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        wav
    }

    #[test]
    fn wav_header_declares_frames_without_loops() {
        let wav = wav_header(4000);
        let size = wav.len() as u64 + 4000;
        assert_eq!(
            probe_bytes(&wav, size),
            Some(Format {
                channels: 2,
                rate: 48_000,
                frames: 1000,
                loops: Vec::new(),
            })
        );
        // ADPCM (tag 2) is not a format the engine reads.
        let mut adpcm = wav.clone();
        adpcm[20] = 2;
        assert_eq!(probe_bytes(&adpcm, size), None);
    }

    fn flac_streaminfo() -> Vec<u8> {
        // STREAMINFO: 48 kHz, 2 channels, 16 bits, 12345 frames.
        let packed: u64 = (48_000 << 44) | (1 << 41) | (15 << 36) | 12_345;
        let mut flac = b"fLaC\x00\x00\x00\x22".to_vec();
        flac.extend([0u8; 10]);
        flac.extend(packed.to_be_bytes());
        flac.extend([0u8; 16]); // the MD5 that ends the 34-byte block
        flac
    }

    fn sampler_chunk(counts: &[u32]) -> Vec<u8> {
        let mut body = vec![0; 36];
        body[28..32].copy_from_slice(&(counts.len() as u32).to_le_bytes());
        for (i, &count) in counts.iter().enumerate() {
            let start = 100 + 100 * i as u32;
            for v in [i as u32, 0, start, start + 99, 0, count] {
                body.extend(v.to_le_bytes());
            }
        }
        let mut chunk = b"smpl".to_vec();
        chunk.extend((body.len() as u32).to_le_bytes());
        chunk.extend(body);
        chunk
    }

    #[test]
    fn loopless_flac_with_last_metadata_header_crossing_64_kib_keeps_its_format() {
        let mut flac = flac_streaminfo();
        flac.extend(b"\x01\x00\xff\xd0"); // PADDING, 65,488 bytes.
        flac.resize(65_534, 0);
        flac.extend(b"\x81\x00\x00\x00"); // Last metadata block, empty PADDING.
        assert_eq!(
            probe_bytes(&flac, flac.len() as u64),
            Some(Format {
                channels: 2,
                rate: 48_000,
                frames: 12_345,
                loops: Vec::new(),
            })
        );
    }

    #[test]
    fn flac_limits_metadata_reads_without_losing_streaminfo() {
        let mut flac = flac_streaminfo();
        flac.extend(b"\x01\x00\x00\x00".repeat(1024));
        flac.extend(b"\x81\x00\x00\x00");
        let mut reads = 0;
        let format = probe(flac.len() as u64, |at, out| {
            reads += 1;
            let start = at as usize;
            out.copy_from_slice(flac.get(start..start + out.len())?);
            Some(())
        });
        assert_eq!(format.map(|f| (f.frames, f.loops.len())), Some((12_345, 0)));
        assert!(reads <= 258, "made {reads} positioned reads");
    }

    #[test]
    fn wav_limits_metadata_reads_after_data_without_losing_audio() {
        let mut wav = wav_header(4);
        wav.extend([0; 4]);
        wav.extend(b"JUNK\0\0\0\0".repeat(1024));
        let mut reads = 0;
        let format = probe(wav.len() as u64, |at, out| {
            reads += 1;
            let start = at as usize;
            out.copy_from_slice(wav.get(start..start + out.len())?);
            Some(())
        });
        assert_eq!(format.map(|f| (f.frames, f.loops.len())), Some((1, 0)));
        assert!(reads <= 261, "made {reads} positioned reads");
    }

    #[test]
    fn flac_finds_loop_after_padding_without_reading_the_padding() {
        for padding in [65_488u32, 65_536] {
            let mut flac = flac_streaminfo();
            flac.push(1); // PADDING
            flac.extend(&padding.to_be_bytes()[1..]);
            let payload = flac.len() as u64;
            let next = payload + u64::from(padding);
            flac.resize(next as usize, 0);
            let smpl = sampler_chunk(&[0]);
            flac.push(0x82); // Last block, APPLICATION.
            flac.extend(&((4 + smpl.len()) as u32).to_be_bytes()[1..]);
            flac.extend(b"riff");
            flac.extend(smpl);
            let format = probe(flac.len() as u64, |at, out| {
                let end = at + out.len() as u64;
                assert!(end <= payload || at >= next, "read padding");
                out.copy_from_slice(flac.get(at as usize..end as usize)?);
                Some(())
            })
            .unwrap();
            assert_eq!(
                (format.channels, format.rate, format.frames),
                (2, 48_000, 12_345)
            );
            assert_eq!(format.loops.len(), 1);
            assert_eq!(
                (
                    format.loops[0].start,
                    format.loops[0].end,
                    format.loops[0].count
                ),
                (100, 200, 0)
            );

            let truncated = &flac[..next as usize + 2];
            let format = probe_bytes(truncated, truncated.len() as u64).unwrap();
            assert_eq!(format.frames, 12_345);
            assert!(format.loops.is_empty());
        }
        assert_eq!(probe_bytes(b"OggS\0\0\0\0", 8), None);
    }

    #[test]
    fn wav_finds_loops_after_data_and_converts_play_counts_without_spilling() {
        for data_len in [4_000, 80_000] {
            let mut wav = wav_header(data_len);
            let audio_start = wav.len() as u64;
            let audio_end = audio_start + u64::from(data_len);
            wav.resize(audio_end as usize, 0);
            wav.extend(sampler_chunk(&[1, 2, 0, 3, 3, 3, 3, 3, 3, 3]));
            let riff_size = (wav.len() - 8) as u32;
            wav[4..8].copy_from_slice(&riff_size.to_le_bytes());
            let format = probe(wav.len() as u64, |at, out| {
                let end = at + out.len() as u64;
                assert!(end <= audio_start || at >= audio_end, "read audio");
                out.copy_from_slice(wav.get(at as usize..end as usize)?);
                Some(())
            })
            .unwrap();
            assert_eq!(format.frames, u64::from(data_len) / 4);
            let loops: Vec<_> = format.loops.iter().map(|l| (l.start, l.count)).collect();
            assert_eq!(
                loops,
                [
                    (200, 2),
                    (300, 0),
                    (400, 3),
                    (500, 3),
                    (600, 3),
                    (700, 3),
                    (800, 3),
                    (900, 3)
                ]
            );
        }
    }
}
