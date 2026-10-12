//! WAV, AIFF and FLAC to stereo frames, bounded as v1's `src/uvi/sample.rs`.

use sampler_kontakt::Decoded;
use std::io::Cursor;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as AudioError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

const MEMORY_LIMIT: usize = 512 << 20;

/// Interleaved samples, their channel count and rate.
pub(crate) fn decode_channels(bytes: &[u8]) -> Result<(Vec<f32>, usize, u32), String> {
    if bytes.is_empty() || bytes.len() > MEMORY_LIMIT {
        return Err("audio is empty or exceeds 512 MiB".into());
    }
    let mut encoded = bytes.to_vec();
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        // Symphonia needs a consistent RIFF extent; banks do not always keep one.
        encoded[4..8].copy_from_slice(&((bytes.len() - 8) as u32).to_le_bytes());
    }
    let stream = MediaSourceStream::new(Box::new(Cursor::new(encoded)), Default::default());
    let metadata = MetadataOptions {
        limit_metadata_bytes: Limit::Maximum(2 << 20),
        limit_visual_bytes: Limit::Maximum(2 << 20),
    };
    let mut format = symphonia::default::get_probe()
        .format(&Hint::new(), stream, &FormatOptions::default(), &metadata)
        .map_err(|e| format!("not WAV, AIFF or FLAC: {e}"))?
        .format;
    let track = format.default_track().ok_or("no audio track")?;
    let (id, params) = (track.id, track.codec_params.clone());
    let rate = params.sample_rate.ok_or("undeclared sample rate")?;
    let channels = params.channels.ok_or("undeclared channel count")?.count();
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(AudioError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let buffer = buffer
            .get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, *decoded.spec()));
        buffer.copy_interleaved_ref(decoded);
        if (samples.len() + buffer.samples().len()) * 4 > MEMORY_LIMIT {
            return Err("decoded audio exceeds 512 MiB".into());
        }
        samples.extend_from_slice(buffer.samples());
    }
    if rate == 0 || channels == 0 || samples.is_empty() {
        return Err("invalid audio dimensions".into());
    }
    Ok((samples, channels, rate))
}

/// One sample, or a channel bundle of mono members, as stereo frames: mono
/// is duplicated and channels past the second are left out (reported by the caller).
pub(crate) fn decode(parts: &[Vec<u8>]) -> Result<(Decoded, usize), String> {
    let decoded = parts
        .iter()
        .map(|bytes| decode_channels(bytes))
        .collect::<Result<Vec<_>, _>>()?;
    let channels: Vec<(Vec<f32>, usize)> = match decoded.as_slice() {
        [(samples, count, _)] => (0..(*count).min(2))
            .map(|c| (samples.iter().skip(c).step_by(*count).copied().collect(), 1))
            .collect(),
        many => {
            if many
                .iter()
                .any(|(_, count, rate)| *count != 1 || *rate != many[0].2)
            {
                return Err("channel bundle members must be mono at one rate".into());
            }
            many.iter()
                .take(2)
                .map(|(samples, _, _)| (samples.clone(), 1))
                .collect()
        }
    };
    let total = decoded.iter().map(|(_, count, _)| count).sum();
    let (left, right) = (&channels[0].0, &channels.get(1).unwrap_or(&channels[0]).0);
    let frames = left.iter().zip(right).map(|(&l, &r)| [l, r]).collect();
    Ok((
        Decoded {
            rate: decoded[0].2,
            frames,
        },
        total,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn mono_wav_becomes_stereo_frames() {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x80\xbb\0\0\0\x77\x01\0\x02\0\x10\0data\x04\0\0\0".to_vec();
        wav.extend_from_slice(&[0, 0x40, 0, 0xc0]);
        let (decoded, channels) = super::decode(&[wav]).unwrap();
        assert_eq!((decoded.rate, channels), (48000, 1));
        assert_eq!(decoded.frames, [[0.5, 0.5], [-0.5, -0.5]]);
    }
}
