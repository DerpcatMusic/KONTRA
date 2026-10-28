//! What a sample file's own header declares: channels, rate and frames for
//! PCM WAV and FLAC. The engine sniffs the codec from the same bytes; the
//! instrument model needs the length up front to place zones.

/// The declared layout of one sample file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Format {
    pub channels: u8,
    pub rate: u32,
    pub frames: u64,
}

/// Reads `head`, the first bytes of a sample of `size` bytes. UVI members
/// hold FLAC behind `.wav` names, so the magic decides, not the name.
pub(crate) fn probe(head: &[u8], size: u64) -> Option<Format> {
    match head.get(..4)? {
        b"fLaC" => flac(head),
        b"RIFF" => wav(head, size),
        _ => None,
    }
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// The STREAMINFO block: its rate, channels and total frames packed after
/// the 10 bytes of block sizes (RFC 9639 §8.2). Zero frames means unknown.
fn flac(head: &[u8]) -> Option<Format> {
    let packed = u64::from_be_bytes(head.get(18..26)?.try_into().ok()?);
    let rate = (packed >> 44) as u32;
    let channels = ((packed >> 41) & 7) as u8 + 1;
    let frames = packed & ((1 << 36) - 1);
    (rate > 0 && frames > 0).then_some(Format {
        channels,
        rate,
        frames,
    })
}

/// The `fmt ` chunk, then the `data` chunk's length in whole frames, clamped to
/// the bytes the member holds (streamed files may declare more). Only PCM and
/// float are read (tag 1 or 3, directly or as the extensible subformat), as
/// the engine's reader.
fn wav(head: &[u8], size: u64) -> Option<Format> {
    if head.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut fmt = None;
    while at + 8 <= head.len() {
        let len = le32(head, at + 4)? as usize;
        let body = at + 8;
        match head.get(at..at + 4)? {
            b"fmt " => {
                let tag = match le16(head, body)? {
                    0xFFFE => le16(head, body + 24)?,
                    tag => tag,
                };
                if tag != 1 && tag != 3 {
                    return None;
                }
                let channels = le16(head, body + 2)?;
                let rate = le32(head, body + 4)?;
                let align = le16(head, body + 12)?;
                fmt = Some((channels, rate, align));
            }
            b"data" => {
                let (channels, rate, align) = fmt?;
                let available = size.saturating_sub(body as u64);
                let bytes = (len as u64).min(available);
                if align == 0 || channels == 0 || channels > 255 {
                    return None;
                }
                return Some(Format {
                    channels: channels as u8,
                    rate,
                    frames: bytes / u64::from(align),
                });
            }
            _ => {}
        }
        at = body.checked_add(len)?.checked_add(len & 1)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_and_flac_headers_declare_their_frames() {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(2u16.to_le_bytes()); // stereo
        wav.extend(48_000u32.to_le_bytes());
        wav.extend(192_000u32.to_le_bytes());
        wav.extend(4u16.to_le_bytes()); // block align
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(4000u32.to_le_bytes());
        let size = wav.len() as u64 + 4000;
        assert_eq!(
            probe(&wav, size),
            Some(Format {
                channels: 2,
                rate: 48_000,
                frames: 1000
            })
        );
        // ADPCM (tag 2) is not a format the engine reads.
        let mut adpcm = wav.clone();
        adpcm[20] = 2;
        assert_eq!(probe(&adpcm, size), None);
        // STREAMINFO: 48 kHz, 2 channels, 16 bits, 12345 frames.
        let packed: u64 = (48_000 << 44) | (1 << 41) | (15 << 36) | 12_345;
        let mut flac = b"fLaC\x00\x00\x00\x22".to_vec();
        flac.extend([0u8; 10]);
        flac.extend(packed.to_be_bytes());
        assert_eq!(
            probe(&flac, 1 << 20),
            Some(Format {
                channels: 2,
                rate: 48_000,
                frames: 12_345
            })
        );
        assert_eq!(probe(b"OggS\0\0\0\0", 8), None);
    }
}
