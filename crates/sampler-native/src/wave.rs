//! Narrow offline WAV boundary: PCM16 / IEEE float32, mono/stereo, RIFF only.
//! Original implementation from format fields, not copied codec code. See NATIVE_ENTRY.md.
use sampler_core::{Frame, Pcm};
use std::io::{self, Read, Write};
use std::path::Path;

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

pub fn read(path: &Path) -> io::Result<Pcm> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err(invalid("resident WAV limit is 256 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("resident WAV limit is 256 MiB"));
    }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> io::Result<Pcm> {
    if bytes.len() < 12
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
        || u64::from(u32le(bytes, 4)) + 8 != bytes.len() as u64
    {
        return Err(invalid("expected a complete RIFF WAVE file"));
    }
    let mut fmt = None;
    let mut data = None;
    let mut at = 12usize;
    while at < bytes.len() {
        let header = bytes
            .get(at..at + 8)
            .ok_or_else(|| invalid("truncated chunk header"))?;
        let begin = at + 8;
        let length = u32le(header, 4) as usize;
        let end = begin
            .checked_add(length)
            .ok_or_else(|| invalid("chunk length overflow"))?;
        let body = bytes
            .get(begin..end)
            .ok_or_else(|| invalid("truncated chunk"))?;
        let target = match &header[..4] {
            b"fmt " => Some(&mut fmt),
            b"data" => Some(&mut data),
            _ => None,
        };
        if let Some(target) = target
            && target.replace(body).is_some()
        {
            return Err(invalid("duplicate format/data chunk unsupported"));
        }
        at = end
            .checked_add(length & 1)
            .ok_or_else(|| invalid("chunk padding overflow"))?;
        if at > bytes.len() {
            return Err(invalid("missing chunk padding"));
        }
    }
    let fmt = fmt.ok_or_else(|| invalid("missing format chunk"))?;
    if !(fmt.len() == 16 || (fmt.len() == 18 && u16le(fmt, 16) == 0)) {
        return Err(invalid("extended WAV formats unsupported"));
    }
    let (encoding, channels, rate, byte_rate, align, bits) = (
        u16le(fmt, 0),
        u16le(fmt, 2),
        u32le(fmt, 4),
        u32le(fmt, 8),
        u16le(fmt, 12),
        u16le(fmt, 14),
    );
    if !matches!((encoding, bits), (1, 16) | (3, 32))
        || !(1..=2).contains(&channels)
        || rate == 0
        || align != channels * (bits / 8)
        || rate.checked_mul(u32::from(align)) != Some(byte_rate)
    {
        return Err(invalid(
            "only valid mono/stereo PCM16 and float32 WAV are supported",
        ));
    }
    let data = data.ok_or_else(|| invalid("missing data chunk"))?;
    if data.is_empty() || data.len() % usize::from(align) != 0 {
        return Err(invalid("empty or partial audio frame"));
    }
    let mut frames = Vec::with_capacity(data.len() / usize::from(align));
    for bytes in data.chunks_exact(usize::from(align)) {
        let sample = |channel: usize| {
            let offset = channel * usize::from(bits / 8);
            if encoding == 1 {
                f32::from(i16::from_le_bytes(
                    bytes[offset..offset + 2].try_into().unwrap(),
                )) / 32768.0
            } else {
                f32::from_bits(u32le(bytes, offset))
            }
        };
        let l = sample(0);
        let r = if channels == 1 { l } else { sample(1) };
        if !l.is_finite() || !r.is_finite() {
            return Err(invalid("nonfinite PCM"));
        }
        frames.push([l, r]);
    }
    Ok(Pcm {
        rate,
        frames: frames.into_boxed_slice(),
    })
}

/// Float32 stereo WAVEFORMATEX, with fact chunk. All IO stays outside rendering.
pub fn header(out: &mut impl Write, rate: u32, frames: usize) -> io::Result<()> {
    let frames = u32::try_from(frames).map_err(|_| invalid("too many frames for RIFF"))?;
    let bytes = frames
        .checked_mul(8)
        .ok_or_else(|| invalid("RIFF size overflow"))?;
    let riff = bytes
        .checked_add(50)
        .ok_or_else(|| invalid("RIFF size overflow"))?;
    let byte_rate = rate
        .checked_mul(8)
        .filter(|_| rate != 0)
        .ok_or_else(|| invalid("invalid output rate"))?;
    out.write_all(b"RIFF")?;
    out.write_all(&riff.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&18u32.to_le_bytes())?;
    out.write_all(&3u16.to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&8u16.to_le_bytes())?;
    out.write_all(&32u16.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(b"fact")?;
    out.write_all(&4u32.to_le_bytes())?;
    out.write_all(&frames.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&bytes.to_le_bytes())
}

pub fn frames(out: &mut impl Write, frames: &[Frame]) -> io::Result<()> {
    for frame in frames {
        for sample in frame {
            out.write_all(&sample.to_le_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wave_roundtrip_and_malformed_boundaries() {
        let pcm = [[-0.5, 0.5], [1.0, -1.0], [0.0, f32::MIN_POSITIVE]];
        let mut bytes = Vec::new();
        header(&mut bytes, 48000, pcm.len()).unwrap();
        frames(&mut bytes, &pcm).unwrap();
        let result = decode(&bytes).unwrap();
        assert_eq!(result.rate, 48000);
        assert_eq!(&*result.frames, &pcm);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut bad = bytes.clone();
        bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad[58..62].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&bad).is_err());
        // Authored canonical PCM16 mono, independent of the float writer.
        let mut mono = b"RIFF".to_vec();
        mono.extend(40u32.to_le_bytes());
        mono.extend(b"WAVEfmt ");
        mono.extend(16u32.to_le_bytes());
        mono.extend(1u16.to_le_bytes());
        mono.extend(1u16.to_le_bytes());
        mono.extend(44100u32.to_le_bytes());
        mono.extend(88200u32.to_le_bytes());
        mono.extend(2u16.to_le_bytes());
        mono.extend(16u16.to_le_bytes());
        mono.extend(b"data");
        mono.extend(4u32.to_le_bytes());
        mono.extend(i16::MIN.to_le_bytes());
        mono.extend(i16::MAX.to_le_bytes());
        assert_eq!(
            &*decode(&mono).unwrap().frames,
            &[[-1.0; 2], [32767.0 / 32768.0; 2]]
        );
    }
}
