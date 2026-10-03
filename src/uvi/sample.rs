//! Bounded offline audio decoding; retain every channel in source order.

use super::storage::Storage;
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::io::{self, Cursor};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as AudioError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

const MEMORY_LIMIT: usize = 256 << 20;
const METADATA_LIMIT: usize = 2 << 20;
const CHUNK_LIMIT: usize = 4096;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SampleLoop {
    pub id: u32,
    pub kind: u32,
    pub start: u32,
    /// RIFF's inclusive last frame, preserved without rounding or conversion.
    pub end: u32,
    pub fraction: u32,
    pub play_count: u32,
}

#[derive(Debug, Serialize)]
pub struct Sample {
    pub rate: u32,
    pub channels: usize,
    pub frames: usize,
    #[serde(skip)]
    pub interleaved: Storage,
    /// Original sampler markers, which may be stale. Validate bounds before playback.
    pub loops: Vec<SampleLoop>,
    /// RIFF unity note is metadata, not evidence of a preset zone's root note.
    pub unity_note: Option<u32>,
    /// RIFF `clm ` cycle-size hint; wavetable consumers validate its geometry.
    pub wavetable_cycle_frames: Option<u32>,
    /// Original FLAC APPLICATION `riff` payloads, including their application ID.
    #[serde(skip)]
    pub riff_metadata: Vec<Vec<u8>>,
}

/// Assemble an ordered mono bundle. All operands must have identical timing
/// and sampler metadata. Ownership is consumed: cache the result, not a second
/// copy of the operands. Larger bundles require a streaming source.
pub fn assemble_mono(mut samples: Vec<Sample>) -> Result<Sample> {
    let first = samples.first().context("Empty mono sample bundle")?;
    let (rate, frames, channels) = (first.rate, first.frames, samples.len());
    ensure!(rate > 0 && frames > 0, "Invalid mono bundle dimensions");
    let count = frames
        .checked_mul(channels)
        .context("Mono bundle sample count overflow")?;
    ensure!(
        count <= MEMORY_LIMIT / size_of::<f32>(),
        "Mono bundle exceeds memory limit"
    );
    let mut metadata_bytes = 0usize;
    let mut metadata_chunks = 0usize;
    for sample in &samples {
        ensure!(sample.channels == 1, "Sample bundle operands must be mono");
        ensure!(
            sample.rate == rate && sample.frames == frames,
            "Mono bundle timing mismatch"
        );
        ensure!(
            sample.loops == first.loops && sample.unity_note == first.unity_note,
            "Mono bundle sampler metadata mismatch"
        );
        ensure!(
            sample.wavetable_cycle_frames == first.wavetable_cycle_frames,
            "Mono bundle wavetable metadata mismatch"
        );
        ensure!(
            sample.interleaved.len() == frames,
            "Mono bundle PCM length mismatch"
        );
        for block in &sample.riff_metadata {
            metadata_bytes = metadata_bytes
                .checked_add(block.len())
                .context("Mono bundle metadata size overflow")?;
            metadata_chunks += 1;
            ensure!(
                metadata_bytes <= METADATA_LIMIT && metadata_chunks <= CHUNK_LIMIT,
                "Mono bundle metadata exceeds limit"
            );
        }
    }
    ensure!(
        first.loops.len() <= CHUNK_LIMIT,
        "Too many mono bundle loops"
    );
    if channels == 1 {
        return Ok(samples.pop().unwrap());
    }
    let loops = first.loops.clone();
    let unity_note = first.unity_note;
    let wavetable_cycle_frames = first.wavetable_cycle_frames;
    let mut interleaved = Vec::new();
    interleaved.try_reserve_exact(count)?;
    interleaved.resize(count, 0.);
    let mut riff_metadata = Vec::new();
    // ponytail: resident assembly uses temporary operand buffers; stream bundles
    // when their combined resident allocation exceeds the caller's RAM budget.
    for (channel, sample) in samples.into_iter().enumerate() {
        for (frame, value) in sample.interleaved.iter().enumerate() {
            interleaved[frame * channels + channel] = value;
        }
        riff_metadata.extend(sample.riff_metadata);
    }
    Ok(Sample {
        rate,
        channels,
        frames,
        interleaved: Storage::from_f32(interleaved)?,
        loops,
        unity_note,
        wavetable_cycle_frames,
        riff_metadata,
    })
}

#[derive(Default)]
struct Metadata {
    loops: Vec<SampleLoop>,
    unity_note: Option<u32>,
    wavetable_cycle_frames: Option<u32>,
    wave_channels: Option<u16>,
    wave_container_bits: Option<u16>,
    riff: Vec<Vec<u8>>,
}

fn u32le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

fn clm_cycle_frames(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (digits, _) = text.strip_prefix("<!>")?.split_once(' ')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

// Original parser using Apple's CAF 1.0 field definitions and LPCM alignment:
// https://developer.apple.com/library/archive/documentation/MusicAudio/Reference/CAFSpec/CAF_spec/CAF_spec.html
fn decode_caf(bytes: &[u8]) -> Result<Sample> {
    ensure!(
        bytes.get(..8) == Some(b"caff\0\x01\0\0"),
        "Unsupported or truncated CAF header"
    );
    let mut at = 8usize;
    let mut chunks = 0usize;
    let mut metadata_bytes = 0usize;
    let mut description = None;
    let mut pcm = None;
    while at < bytes.len() {
        let header = bytes
            .get(at..at + 12)
            .context("Truncated CAF chunk header")?;
        let length = i64::from_be_bytes(header[4..12].try_into().unwrap());
        ensure!(length >= 0, "Indefinite CAF chunk lengths are unsupported");
        let length = usize::try_from(length).context("CAF chunk size exceeds address space")?;
        let end = (at + 12)
            .checked_add(length)
            .context("CAF chunk size overflow")?;
        let body = bytes.get(at + 12..end).context("Truncated CAF chunk")?;
        chunks += 1;
        ensure!(chunks <= CHUNK_LIMIT, "CAF exceeds chunk limit");
        ensure!(
            chunks != 1 || &header[..4] == b"desc",
            "CAF description must be the first chunk"
        );
        if &header[..4] != b"data" {
            metadata_bytes = metadata_bytes
                .checked_add(length)
                .context("CAF metadata size overflow")?;
            ensure!(
                metadata_bytes <= METADATA_LIMIT,
                "CAF metadata exceeds limit"
            );
        }
        match &header[..4] {
            b"desc" => {
                ensure!(
                    description.is_none() && body.len() == 32,
                    "Invalid or duplicate CAF description"
                );
                let rate = f64::from_bits(u64::from_be_bytes(body[..8].try_into().unwrap()));
                ensure!(
                    rate.is_finite()
                        && rate > 0.
                        && rate <= f64::from(u32::MAX)
                        && rate.fract() == 0.,
                    "CAF sample rate must be a positive integer supported by the engine"
                );
                ensure!(&body[8..12] == b"lpcm", "Only CAF LPCM is supported");
                let read = |at| u32::from_be_bytes(body[at..at + 4].try_into().unwrap());
                let (flags, packet_bytes, packet_frames, channels, bits) =
                    (read(12), read(16), read(20), read(24), read(28));
                ensure!(flags & !3 == 0, "Unsupported CAF LPCM flags");
                ensure!(
                    packet_frames == 1
                        && channels > 0
                        && packet_bytes > 0
                        && packet_bytes % channels == 0,
                    "Unsupported CAF LPCM packet geometry"
                );
                let width = (packet_bytes / channels) as usize;
                let floating = flags & 1 != 0;
                ensure!(
                    if floating {
                        matches!((bits, width), (32, 4) | (64, 8))
                    } else {
                        matches!((bits, width), (16, 2) | (24, 3) | (24, 4) | (32, 4))
                    },
                    "Unsupported CAF LPCM sample width"
                );
                description = Some((
                    rate as u32,
                    channels as usize,
                    packet_bytes as usize,
                    bits,
                    width,
                    floating,
                    flags & 2 != 0,
                ));
            }
            b"data" => {
                ensure!(
                    pcm.is_none() && body.len() >= 4,
                    "Invalid or duplicate CAF data chunk"
                );
                pcm = Some(&body[4..]); // The edit count is not audio data.
            }
            _ => {}
        }
        at = end;
    }
    let (rate, channels, frame_bytes, bits, width, floating, little) =
        description.context("Missing CAF description")?;
    let pcm = pcm.context("Missing CAF data")?;
    ensure!(
        !pcm.is_empty() && pcm.len() % frame_bytes == 0,
        "Partial or empty CAF PCM frame"
    );
    let frames = pcm.len() / frame_bytes;
    let count = frames
        .checked_mul(channels)
        .context("CAF sample count overflow")?;
    ensure!(
        count <= (MEMORY_LIMIT - bytes.len()) / size_of::<f32>(),
        "Decoded CAF exceeds memory limit"
    );
    let mut interleaved = Vec::new();
    interleaved.try_reserve_exact(count)?;
    for sample in pcm.chunks_exact(width) {
        let word = if little {
            sample
                .iter()
                .rev()
                .fold(0u64, |word, &byte| (word << 8) | u64::from(byte))
        } else {
            sample
                .iter()
                .fold(0u64, |word, &byte| (word << 8) | u64::from(byte))
        };
        let value = if floating {
            if width == 4 {
                f32::from_bits(word as u32)
            } else {
                f64::from_bits(word) as f32
            }
        } else {
            let padding = width * 8 - bits as usize;
            ensure!(
                padding == 0 || word & ((1u64 << padding) - 1) == 0,
                "Nonzero CAF LPCM padding bits"
            );
            let shift = 64 - width * 8;
            let integer = ((word << shift) as i64) >> shift;
            integer as f32 / 2f32.powi((width * 8 - 1) as i32)
        };
        interleaved.push(value);
    }
    Ok(Sample {
        rate,
        channels,
        frames,
        interleaved: Storage::from_f32(interleaved)?,
        loops: Vec::new(),
        unity_note: None,
        wavetable_cycle_frames: None,
        riff_metadata: Vec::new(),
    })
}

fn sampler(bytes: &[u8], metadata: &mut Metadata) -> Result<()> {
    ensure!(bytes.len() >= 36, "Truncated RIFF sampler metadata");
    let loops = u32le(&bytes[28..]) as usize;
    let extra = u32le(&bytes[32..]) as usize;
    ensure!(loops <= CHUNK_LIMIT, "Too many RIFF sample loops");
    ensure!(
        36usize
            .checked_add(loops * 24)
            .and_then(|n| n.checked_add(extra))
            == Some(bytes.len()),
        "Invalid RIFF sampler metadata length"
    );
    ensure!(
        metadata.unity_note.is_none(),
        "Duplicate RIFF sampler metadata"
    );
    metadata.unity_note = Some(u32le(&bytes[12..]));
    for entry in bytes[36..36 + loops * 24].chunks_exact(24) {
        metadata.loops.push(SampleLoop {
            id: u32le(entry),
            kind: u32le(&entry[4..]),
            start: u32le(&entry[8..]),
            end: u32le(&entry[12..]),
            fraction: u32le(&entry[16..]),
            play_count: u32le(&entry[20..]),
        });
    }
    Ok(())
}

// Validate lengths before the demuxer can allocate from untrusted chunk headers.
fn metadata(bytes: &[u8]) -> Result<Metadata> {
    let mut result = Metadata::default();
    let mut total = 0usize;
    let mut chunks = 0usize;
    if bytes.starts_with(b"fLaC") {
        let mut at = 4usize;
        loop {
            let header = bytes
                .get(at..at + 4)
                .context("Truncated FLAC metadata header")?;
            let length =
                ((header[1] as usize) << 16) | ((header[2] as usize) << 8) | header[3] as usize;
            total += length;
            chunks += 1;
            ensure!(
                total <= METADATA_LIMIT && chunks <= CHUNK_LIMIT,
                "FLAC metadata exceeds limit"
            );
            let body = bytes
                .get(at + 4..at + 4 + length)
                .context("Truncated FLAC metadata")?;
            if chunks == 1 {
                ensure!(
                    header[0] & 127 == 0 && length == 34,
                    "Missing FLAC STREAMINFO"
                );
            }
            if header[0] & 127 == 2 {
                ensure!(length >= 4, "Truncated FLAC application ID");
                if body.starts_with(b"riff") {
                    result.riff.push(body.to_vec());
                    let chunk = &body[4..];
                    ensure!(
                        chunk.len() >= 8 || chunk.starts_with(b"clm "),
                        "Truncated FLAC RIFF application"
                    );
                    if chunk.starts_with(b"smpl") {
                        let length = u32le(&chunk[4..]) as usize;
                        ensure!(length == chunk.len() - 8, "Truncated FLAC RIFF sampler");
                        sampler(&chunk[8..], &mut result)?;
                    } else if chunk.starts_with(b"clm ") {
                        let hint = chunk.get(4..8).and_then(|header| {
                            let end = 8usize.checked_add(u32le(header) as usize)?;
                            clm_cycle_frames(chunk.get(8..end)?)
                        });
                        result.wavetable_cycle_frames = result.wavetable_cycle_frames.or(hint);
                    }
                }
            }
            at += 4 + length;
            if header[0] & 128 != 0 {
                ensure!(at < bytes.len(), "FLAC has no audio frames");
                break;
            }
        }
    } else {
        ensure!(bytes.len() >= 12, "Truncated audio container header");
        let wave = bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE";
        let aiff = bytes.starts_with(b"FORM") && matches!(&bytes[8..12], b"AIFF" | b"AIFC");
        ensure!(wave || aiff, "Expected WAV, AIFF or FLAC audio");
        let length = |b: &[u8]| if wave { u32le(b) } else { u32::from_be_bytes(b[..4].try_into().unwrap()) } as usize;
        // Native WAV loading accepts inaccurate outer RIFF extents. Every
        // inner chunk is still bounded to physical bytes before probing.
        if aiff {
            ensure!(
                length(&bytes[4..]) == bytes.len() - 8,
                "Audio container length mismatch"
            );
        }
        let mut at = 12usize;
        while at < bytes.len() {
            let header = bytes
                .get(at..at + 8)
                .context("Truncated audio chunk header")?;
            let size = length(&header[4..]);
            let end = (at + 8)
                .checked_add(size)
                .context("Audio chunk size overflow")?;
            let body = bytes.get(at + 8..end).context("Truncated audio chunk")?;
            chunks += 1;
            if &header[..4] != b"data" && &header[..4] != b"SSND" {
                total = total
                    .checked_add(size)
                    .context("Audio metadata size overflow")?;
            }
            ensure!(
                total <= METADATA_LIMIT && chunks <= CHUNK_LIMIT,
                "Audio metadata exceeds limit"
            );
            if wave && &header[..4] == b"smpl" {
                sampler(body, &mut result)?;
            } else if wave && &header[..4] == b"clm " {
                result.wavetable_cycle_frames =
                    result.wavetable_cycle_frames.or(clm_cycle_frames(body));
            } else if wave && &header[..4] == b"fmt " {
                let channels = body.get(2..4).context("Truncated WAV channel count")?;
                let channels = u16::from_le_bytes(channels.try_into().unwrap());
                if channels > 32 {
                    ensure!(body.len() >= 16, "Truncated many-channel WAV format");
                    let alignment = u16::from_le_bytes(body[12..14].try_into().unwrap());
                    ensure!(
                        alignment > 0 && alignment % channels == 0,
                        "Invalid many-channel WAV block alignment"
                    );
                    result
                        .wave_container_bits
                        .get_or_insert(u16::from_le_bytes(body[14..16].try_into().unwrap()));
                }
                result.wave_channels.get_or_insert(channels);
            }
            at = end
                .checked_add(size & 1)
                .context("Audio chunk padding overflow")?;
            ensure!(at <= bytes.len(), "Missing audio chunk padding");
        }
    }
    Ok(result)
}

/// Decode an entire asset without downmixing. Encoded + decoded data must fit
/// 256 MiB; metadata is limited to 2 MiB and 4096 chunks. Callers must also bound
/// the total across assets. Undeclared lengths and partial decodes are rejected.
pub fn decode(bytes: &[u8]) -> Result<Sample> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MEMORY_LIMIT,
        "Audio input exceeds memory limit or is empty"
    );
    if bytes.starts_with(b"caff") {
        return decode_caf(bytes);
    }
    let metadata = metadata(bytes)?;
    let mut encoded = bytes.to_vec();
    if bytes.starts_with(b"RIFF") {
        // Symphonia requires a consistent parent extent even though the native
        // reader ignores it. Only the already validated private copy is changed.
        encoded[4..8].copy_from_slice(&((bytes.len() - 8) as u32).to_le_bytes());
    }
    if metadata.wave_channels.is_some_and(|channels| channels > 32) {
        // Symphonia represents layouts with a 32-bit channel mask. Wavetable
        // resources can have more literal channels; hound preserves their order.
        let mut reader = hound::WavReader::new(Cursor::new(encoded))?;
        let spec = reader.spec();
        // ponytail: hound interprets padded integer PCM as right-aligned; reject
        // that variant until a caller needs a separate left-aligned decoder.
        ensure!(
            spec.sample_format != hound::SampleFormat::Int
                || Some(spec.bits_per_sample) == metadata.wave_container_bits,
            "Padded many-channel WAV integer containers are unsupported"
        );
        let channels = usize::from(spec.channels);
        let frames = reader.duration() as usize;
        let count = reader.len() as usize;
        ensure!(
            spec.sample_rate > 0 && frames > 0 && Some(spec.channels) == metadata.wave_channels,
            "Invalid many-channel WAV dimensions"
        );
        ensure!(
            frames.checked_mul(channels) == Some(count),
            "Many-channel WAV frame count mismatch"
        );
        ensure!(
            count <= (MEMORY_LIMIT - bytes.len()) / size_of::<f32>(),
            "Decoded audio exceeds memory limit"
        );
        let mut interleaved = Vec::new();
        interleaved.try_reserve_exact(count)?;
        match spec.sample_format {
            hound::SampleFormat::Float => {
                for value in reader.samples::<f32>() {
                    interleaved.push(value?);
                }
            }
            hound::SampleFormat::Int => {
                ensure!(
                    (1..=32).contains(&spec.bits_per_sample),
                    "Unsupported many-channel WAV integer depth"
                );
                let scale = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
                for value in reader.samples::<i32>() {
                    interleaved.push(value? as f32 / scale);
                }
            }
        }
        ensure!(interleaved.len() == count, "Truncated many-channel WAV PCM");
        return Ok(Sample {
            rate: spec.sample_rate,
            channels,
            frames,
            interleaved: Storage::from_f32(interleaved)?,
            loops: metadata.loops,
            unity_note: metadata.unity_note,
            wavetable_cycle_frames: metadata.wavetable_cycle_frames,
            riff_metadata: metadata.riff,
        });
    }
    let stream = MediaSourceStream::new(Box::new(Cursor::new(encoded)), Default::default());
    let mut format = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            stream,
            &FormatOptions::default(),
            &MetadataOptions {
                limit_metadata_bytes: Limit::Maximum(METADATA_LIMIT),
                limit_visual_bytes: Limit::Maximum(METADATA_LIMIT),
            },
        )?
        .format;
    let track = format.default_track().context("No audio track")?;
    let rate = track
        .codec_params
        .sample_rate
        .context("Undeclared sample rate")?;
    let channels = track
        .codec_params
        .channels
        .context("Undeclared channel count")?
        .count();
    let frames = usize::try_from(
        track
            .codec_params
            .n_frames
            .context("Undeclared frame count")?,
    )?;
    ensure!(
        rate > 0 && channels > 0 && frames > 0,
        "Invalid audio dimensions"
    );
    let count = frames
        .checked_mul(channels)
        .context("Audio sample count overflow")?;
    ensure!(
        count <= (MEMORY_LIMIT - bytes.len()) / size_of::<f32>(),
        "Decoded audio exceeds memory limit"
    );
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions { verify: true })?;
    let track_id = track.id;
    let mut interleaved = Vec::new();
    interleaved.try_reserve_exact(count)?;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(AudioError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != track_id {
            bail!("Unexpected additional audio track");
        }
        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        ensure!(
            spec.rate == rate && spec.channels.count() == channels,
            "Audio dimensions changed during decode"
        );
        ensure!(
            decoded.capacity() <= MEMORY_LIMIT / channels / size_of::<f32>(),
            "Audio packet exceeds memory limit"
        );
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let samples = buffer.samples();
        ensure!(
            samples.len() <= count - interleaved.len(),
            "Decoded more frames than declared"
        );
        ensure!(
            samples.iter().all(|sample| sample.is_finite()),
            "Nonfinite audio sample"
        );
        interleaved.extend_from_slice(samples);
    }
    ensure!(
        interleaved.len() == count,
        "Truncated audio: decoded {} of {frames} frames",
        interleaved.len() / channels
    );
    ensure!(
        decoder.finalize().verify_ok != Some(false),
        "Audio checksum verification failed"
    );
    Ok(Sample {
        rate,
        channels,
        frames,
        interleaved: Storage::from_f32(interleaved)?,
        loops: metadata.loops,
        unity_note: metadata.unity_note,
        wavetable_cycle_frames: metadata.wavetable_cycle_frames,
        riff_metadata: metadata.riff,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caf_lpcm_retains_channel_order_and_rejects_bad_geometry() {
        let fixture = |bits: u32, width: usize, floating: bool, little: bool| {
            let mut pcm = Vec::new();
            let mut expected = Vec::new();
            for index in 0..12 {
                let (word, value) = if floating {
                    let value = [-0., -1., 0.125, 0.5, 0.123456789, -0.25][index % 6];
                    (
                        if width == 4 {
                            (value as f32).to_bits() as u64
                        } else {
                            f64::to_bits(value)
                        },
                        value as f32,
                    )
                } else {
                    let limit = 1i64 << (bits - 1);
                    let value = [-limit, -128, -1, 0, 1, limit - 1][index % 6];
                    (
                        (value as u64) << (width * 8 - bits as usize),
                        value as f32 / 2f32.powi((bits - 1) as i32),
                    )
                };
                if little {
                    pcm.extend_from_slice(&word.to_le_bytes()[..width]);
                } else {
                    pcm.extend_from_slice(&word.to_be_bytes()[8 - width..]);
                }
                expected.push(value);
            }
            let mut bytes = b"caff\0\x01\0\0desc".to_vec();
            bytes.extend_from_slice(&32i64.to_be_bytes());
            bytes.extend_from_slice(&48000f64.to_be_bytes());
            bytes.extend_from_slice(b"lpcm");
            bytes
                .extend_from_slice(&(u32::from(floating) | (u32::from(little) << 1)).to_be_bytes());
            bytes.extend_from_slice(&((width * 4) as u32).to_be_bytes());
            bytes.extend_from_slice(&1u32.to_be_bytes());
            bytes.extend_from_slice(&4u32.to_be_bytes());
            bytes.extend_from_slice(&bits.to_be_bytes());
            // CAF metadata chunks have no RIFF-style even-byte padding.
            bytes.extend_from_slice(b"free");
            bytes.extend_from_slice(&5i64.to_be_bytes());
            bytes.extend_from_slice(&[0; 5]);
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&((pcm.len() + 4) as i64).to_be_bytes());
            bytes.extend_from_slice(&0u32.to_be_bytes());
            bytes.extend_from_slice(&pcm);
            (bytes, expected)
        };
        for (bits, width, floating) in [
            (16, 2, false),
            (24, 3, false),
            (24, 4, false),
            (32, 4, false),
            (32, 4, true),
            (64, 8, true),
        ] {
            for little in [false, true] {
                let (bytes, expected) = fixture(bits, width, floating, little);
                let decoded = decode(&bytes).unwrap();
                assert_eq!(
                    (decoded.rate, decoded.channels, decoded.frames),
                    (48000, 4, 3)
                );
                assert_eq!(decoded.interleaved.len(), 12);
                assert!(
                    decoded
                        .interleaved
                        .iter()
                        .zip(expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
                assert!(decode(&bytes[..bytes.len() - 1]).is_err());
            }
        }
        let (valid, _) = fixture(24, 3, false, false);
        for (at, replacement) in [
            (28, b"ima4".as_slice()),
            (32, &4u32.to_be_bytes()),
            (40, &0u32.to_be_bytes()),
            (44, &0u32.to_be_bytes()),
            (20, &f64::NAN.to_be_bytes()),
            (73, &(-1i64).to_be_bytes()),
        ] {
            let mut malformed = valid.clone();
            malformed[at..at + replacement.len()].copy_from_slice(replacement);
            assert!(decode(&malformed).is_err());
        }
        let mut partial = valid.clone();
        partial.pop();
        partial[73..81].copy_from_slice(&39i64.to_be_bytes());
        assert!(decode(&partial).is_err());
        let mut duplicate = valid.clone();
        duplicate.extend_from_slice(&valid[8..52]);
        assert!(decode(&duplicate).is_err());
        let mut duplicate = valid.clone();
        duplicate.extend_from_slice(&valid[69..]);
        assert!(decode(&duplicate).is_err());
        let (mut padded, _) = fixture(24, 4, false, false);
        *padded.last_mut().unwrap() = 1;
        assert!(decode(&padded).is_err());
        let (mut nonfinite, _) = fixture(32, 4, true, true);
        nonfinite[85..89].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&nonfinite).is_err());
    }

    #[test]
    fn literal_many_channel_pcm_retains_every_table_slice() {
        for channels in [65, 87] {
            for (bits, format) in [
                (16, hound::SampleFormat::Int),
                (24, hound::SampleFormat::Int),
                (32, hound::SampleFormat::Float),
            ] {
                let mut output = Cursor::new(Vec::new());
                let mut expected = Vec::new();
                {
                    let mut writer = hound::WavWriter::new(
                        &mut output,
                        hound::WavSpec {
                            channels,
                            sample_rate: 8000,
                            bits_per_sample: bits,
                            sample_format: format,
                        },
                    )
                    .unwrap();
                    for frame in 0..3 {
                        for channel in 0..channels {
                            let integer = frame * 1000 + i32::from(channel) - 128;
                            let value = if format == hound::SampleFormat::Int {
                                writer.write_sample(integer).unwrap();
                                integer as f32 / 2f32.powi(i32::from(bits) - 1)
                            } else {
                                let value = if frame == 0 && channel == 0 {
                                    -0.
                                } else {
                                    integer as f32 / 4096.
                                };
                                writer.write_sample(value).unwrap();
                                value
                            };
                            expected.push(value);
                        }
                    }
                    writer.finalize().unwrap();
                }
                let mut bytes = output.into_inner();
                if bytes.len() % 2 != 0 {
                    bytes.push(0);
                    let size = (bytes.len() - 8) as u32;
                    bytes[4..8].copy_from_slice(&size.to_le_bytes());
                }
                let decoded = decode(&bytes).unwrap();
                assert_eq!(
                    (decoded.rate, decoded.channels, decoded.frames),
                    (8000, channels as usize, 3)
                );
                assert!(
                    decoded
                        .interleaved
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
                assert!(decode(&bytes[..bytes.len() - 1]).is_err());
                assert!(decode(&bytes[..bytes.len() - 2]).is_err());
                // The same original PCM with plain fmt16 rather than WAVEFORMATEXTENSIBLE.
                assert_eq!(&bytes[12..16], b"fmt ");
                assert_eq!(u32le(&bytes[16..]), 40);
                let mut plain = bytes[..36].to_vec();
                plain[16..20].copy_from_slice(&16u32.to_le_bytes());
                plain[20..22].copy_from_slice(
                    &(if format == hound::SampleFormat::Int {
                        1u16
                    } else {
                        3
                    })
                    .to_le_bytes(),
                );
                plain.extend_from_slice(&bytes[60..]);
                let size = (plain.len() - 8) as u32;
                plain[4..8].copy_from_slice(&size.to_le_bytes());
                let decoded = decode(&plain).unwrap();
                assert!(
                    decoded
                        .interleaved
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
                let mut alignment = bytes.clone();
                alignment[32..34].copy_from_slice(&1u16.to_le_bytes());
                assert!(decode(&alignment).is_err());
                let mut descriptor = bytes.clone();
                descriptor[36..38].copy_from_slice(&21u16.to_le_bytes());
                assert!(decode(&descriptor).is_err());
                if bits == 24 {
                    let mut padded = bytes[..68].to_vec();
                    padded[28..32]
                        .copy_from_slice(&(8000u32 * u32::from(channels) * 4).to_le_bytes());
                    padded[32..34].copy_from_slice(&(channels * 4).to_le_bytes());
                    padded[34..36].copy_from_slice(&32u16.to_le_bytes());
                    padded[64..68].copy_from_slice(&((expected.len() * 4) as u32).to_le_bytes());
                    let end = 68 + u32le(&bytes[64..]) as usize;
                    for word in bytes[68..end].chunks_exact(3) {
                        let value = (u32::from(word[0]) << 8)
                            | (u32::from(word[1]) << 16)
                            | (u32::from(word[2]) << 24);
                        padded.extend_from_slice(&value.to_le_bytes());
                    }
                    let size = (padded.len() - 8) as u32;
                    padded[4..8].copy_from_slice(&size.to_le_bytes());
                    assert!(decode(&padded).unwrap_err().to_string().contains("Padded"));
                }
                if format == hound::SampleFormat::Float {
                    let mut nonfinite = plain;
                    nonfinite[44..48].copy_from_slice(&f32::NAN.to_le_bytes());
                    assert!(decode(&nonfinite).is_err());
                }
            }
        }
    }

    #[test]
    fn ordered_mono_bundle_preserves_identity_and_rejects_mismatches() {
        let mono = |value: f32| Sample {
            rate: 44100,
            channels: 1,
            frames: 3,
            interleaved: Storage::from_f32(vec![value, value + 0.01, value + 0.02]).unwrap(),
            loops: vec![SampleLoop {
                id: 7,
                kind: 0,
                start: 1,
                end: 2,
                fraction: 0,
                play_count: 0,
            }],
            unity_note: Some(64),
            wavetable_cycle_frames: None,
            riff_metadata: vec![vec![value as u8]],
        };
        for channels in [2, 10, 12] {
            let bundle = assemble_mono((0..channels).map(|i| mono(i as f32)).collect()).unwrap();
            assert_eq!(
                (bundle.rate, bundle.channels, bundle.frames),
                (44100, channels, 3)
            );
            for frame in 0..3 {
                for channel in 0..channels {
                    assert_eq!(
                        bundle
                            .interleaved
                            .value(frame * channels + channel)
                            .unwrap(),
                        channel as f32 + frame as f32 * 0.01
                    );
                }
            }
            assert_eq!(bundle.loops, mono(0.).loops);
            assert_eq!(bundle.unity_note, Some(64));
            assert_eq!(
                bundle.riff_metadata,
                (0..channels).map(|i| vec![i as u8]).collect::<Vec<_>>()
            );
        }
        let mut stale_left = mono(0.);
        stale_left.loops[0].end = 5;
        let mut stale_right = mono(1.);
        stale_right.loops[0].end = 5;
        let stale = assemble_mono(vec![stale_left, stale_right]).unwrap();
        assert_eq!(stale.loops[0].end, 5);
        assert_eq!(stale.interleaved.len(), 6);
        assert!(assemble_mono(Vec::new()).is_err());
        let mut mismatches = vec![
            mono(1.),
            mono(1.),
            mono(1.),
            mono(1.),
            mono(1.),
            mono(1.),
            mono(1.),
        ];
        mismatches[0].rate = 48000;
        mismatches[1].frames = 2;
        mismatches[2].channels = 2;
        mismatches[3].loops[0].fraction = 1;
        mismatches[4].unity_note = Some(60);
        mismatches[5].interleaved = Storage::from_f32(vec![0.; 2]).unwrap();
        mismatches[6].wavetable_cycle_frames = Some(2048);
        for invalid in mismatches {
            assert!(assemble_mono(vec![mono(0.), invalid]).is_err());
        }
        let mut overflow = mono(0.);
        overflow.frames = usize::MAX;
        let mut other = mono(1.);
        other.frames = usize::MAX;
        assert!(
            assemble_mono(vec![overflow, other])
                .unwrap_err()
                .to_string()
                .contains("overflow")
        );
        let mut capped = mono(0.);
        capped.frames = MEMORY_LIMIT;
        assert!(
            assemble_mono(vec![capped])
                .unwrap_err()
                .to_string()
                .contains("memory limit")
        );
    }

    #[test]
    fn preserves_six_channels_and_rejects_invalid_audio() {
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut wav,
                hound::WavSpec {
                    channels: 6,
                    sample_rate: 44100,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for frame in 0..16 {
                for channel in 0..6 {
                    writer
                        .write_sample((frame * 60 + channel * 10) as i16)
                        .unwrap();
                }
            }
            writer.finalize().unwrap();
        }
        let bytes = wav.into_inner();
        let sample = decode(&bytes).unwrap();
        assert_eq!(
            (sample.rate, sample.channels, sample.frames),
            (44100, 6, 16)
        );
        for frame in 0..16 {
            for channel in 0..6 {
                assert_eq!(
                    sample.interleaved.value(frame * 6 + channel).unwrap(),
                    (frame * 60 + channel * 10) as f32 / 32768.
                );
            }
        }
        let clm_fixture = |hint: &[u8]| {
            let mut body = hint.to_vec();
            body.resize(48, b' ');
            let mut with_hint = bytes.clone();
            with_hint.extend_from_slice(b"clm ");
            with_hint.extend_from_slice(&48u32.to_le_bytes());
            with_hint.extend_from_slice(&body);
            let size = (with_hint.len() - 8) as u32;
            with_hint[4..8].copy_from_slice(&size.to_le_bytes());
            (with_hint, body)
        };
        let (with_hint, clm) = clm_fixture(b"<!>2048 00000000");
        let hinted = decode(&with_hint).unwrap();
        assert_eq!(hinted.wavetable_cycle_frames, Some(2048));
        assert_eq!(
            hinted.interleaved.to_vec().unwrap(),
            sample.interleaved.to_vec().unwrap()
        );
        for malformed in [
            b"<!>word 0".as_slice(),
            b"<!>+2048 0",
            b"<!>4294967296 0",
            b"2048 0",
            b"<!>2048 \xff",
        ] {
            let invalid = decode(&clm_fixture(malformed).0).unwrap();
            assert_eq!(invalid.wavetable_cycle_frames, None);
        }
        assert_eq!(clm_cycle_frames(b"<!>2048"), None);
        // Geometry is a wavetable-use decision, not a reason to reject ordinary PCM.
        assert_eq!(
            decode(&clm_fixture(b"<!>0 0").0)
                .unwrap()
                .wavetable_cycle_frames,
            Some(0)
        );
        // Original synthetic metadata-only FLAC exercises foreign RIFF preservation.
        let mut flac_metadata = b"fLaC\0\0\0\x22".to_vec();
        flac_metadata.extend_from_slice(&[0; 34]);
        flac_metadata.extend_from_slice(&[0x82, 0, 0, 60]);
        flac_metadata.extend_from_slice(b"riffclm ");
        flac_metadata.extend_from_slice(&48u32.to_le_bytes());
        flac_metadata.extend_from_slice(&clm);
        flac_metadata.push(0);
        let foreign = metadata(&flac_metadata).unwrap();
        assert_eq!(foreign.wavetable_cycle_frames, Some(2048));
        assert_eq!(foreign.riff.len(), 1);
        flac_metadata[54..58].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(
            metadata(&flac_metadata).unwrap().wavetable_cycle_frames,
            None
        );
        // Original AIFF fixture with the same six independently identifiable channels.
        let mut aiff = b"FORM\0\0\0\0AIFFCOMM\0\0\0\x12".to_vec();
        aiff.extend_from_slice(&6u16.to_be_bytes());
        aiff.extend_from_slice(&16u32.to_be_bytes());
        aiff.extend_from_slice(&16u16.to_be_bytes());
        aiff.extend_from_slice(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
        aiff.extend_from_slice(b"SSND");
        aiff.extend_from_slice(&200u32.to_be_bytes());
        aiff.extend_from_slice(&[0; 8]);
        for i in 0..96 {
            aiff.extend_from_slice(&((i / 6 * 60 + i % 6 * 10) as i16).to_be_bytes());
        }
        let size = (aiff.len() - 8) as u32;
        aiff[4..8].copy_from_slice(&size.to_be_bytes());
        let decoded = decode(&aiff).unwrap();
        assert_eq!(
            (decoded.rate, decoded.channels, decoded.frames),
            (44100, 6, 16)
        );
        assert_eq!(
            decoded.interleaved.to_vec().unwrap(),
            sample.interleaved.to_vec().unwrap()
        );
        assert!(decode(&aiff[..aiff.len() - 1]).is_err());
        let mut looped = bytes.clone();
        looped.extend_from_slice(b"smpl");
        looped.extend_from_slice(&60u32.to_le_bytes());
        for word in [0u32, 0, 0, 64, 0, 0, 0, 1, 0, 7, 1, 3, 9, 123, 2] {
            looped.extend_from_slice(&word.to_le_bytes());
        }
        let size = (looped.len() - 8) as u32;
        looped[4..8].copy_from_slice(&size.to_le_bytes());
        let metadata = decode(&looped).unwrap();
        assert_eq!(metadata.unity_note, Some(64));
        assert_eq!(
            metadata.loops,
            [SampleLoop {
                id: 7,
                kind: 1,
                start: 3,
                end: 9,
                fraction: 123,
                play_count: 2,
            }]
        );
        let end = looped.len() - 12;
        looped[end..end + 4].copy_from_slice(&16u32.to_le_bytes());
        let stale = decode(&looped).unwrap();
        assert_eq!(stale.loops[0].end, 16);
        assert_eq!(
            stale.interleaved.to_vec().unwrap(),
            sample.interleaved.to_vec().unwrap()
        );

        let mut float = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut float,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 48000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            writer.write_sample(f32::NAN).unwrap();
            writer.finalize().unwrap();
        }
        assert!(decode(&float.into_inner()).is_err());
        assert!(decode(b"fLaC\x80\x20\x00\x01").is_err());
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        for delta in [4, 8, 12, 128, -4, -128] {
            let mut extent = bytes.clone();
            let size = u32le(&bytes[4..]) as i64 + delta;
            extent[4..8].copy_from_slice(&(size as u32).to_le_bytes());
            assert_eq!(
                decode(&extent).unwrap().interleaved.to_vec().unwrap(),
                sample.interleaved.to_vec().unwrap()
            );
        }
        let mut malformed = bytes.clone();
        malformed[..4].copy_from_slice(b"bad!");
        assert!(decode(&malformed).is_err());
        let mut chunk = bytes;
        chunk[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&chunk).is_err());
        assert!(decode(&[]).is_err());
    }
}
