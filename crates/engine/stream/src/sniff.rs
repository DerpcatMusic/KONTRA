//! What a sample file holds, read from its header: WAV, AIFF/AIFC or NCW.
//! Formats only say where the bytes are; the codec is the file's own.

use std::io;

/// A stored PCM layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pcm {
    pub bits: u8,
    pub float: bool,
    pub big_endian: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Interleaved PCM at `offset..offset + len` of the region.
    Pcm {
        offset: u64,
        len: u64,
        channels: u8,
        pcm: Pcm,
    },
    Ncw,
    /// A FLAC stream from the start of the region.
    Flac,
}

/// Reads the header through `read(pos, out)` over a region of `size` bytes.
pub(crate) fn sniff(
    size: u64,
    read: impl Fn(u64, &mut [u8]) -> io::Result<()>,
) -> io::Result<Layout> {
    let read = |pos: u64, out: &mut [u8]| {
        if pos + out.len() as u64 > size {
            return Err(invalid("header past the end of the sample"));
        }
        read(pos, out)
    };
    let mut magic = [0; 12];
    read(0, &mut magic)?;
    match magic.get(..4) {
        Some(b"RIFF") => wav(size, read),
        Some(b"FORM") => aiff(size, magic.get(8..12) == Some(b"AIFC"), read),
        Some(b"fLaC") => Ok(Layout::Flac),
        _ => {
            let mut head = [0; 120];
            let n = size.min(120) as usize;
            read(0, head.get_mut(..n).unwrap_or_default())?;
            ncw::NcwHeader::read(&mut head.get(..n).unwrap_or_default())
                .map_err(|e| invalid(&format!("not WAV, AIFF, FLAC or NCW: {e}")))?;
            Ok(Layout::Ncw)
        }
    }
}

fn wav(size: u64, read: impl Fn(u64, &mut [u8]) -> io::Result<()>) -> io::Result<Layout> {
    let mut pos = 12;
    let mut format = None;
    while pos + 8 <= size {
        let mut head = [0; 8];
        read(pos, &mut head)?;
        let len = u64::from(u32::from_le_bytes(at_n(&head, 4)));
        match head.get(..4) {
            Some(b"fmt ") => {
                let mut f = [0; 40];
                let n = len.min(40) as usize;
                read(pos + 8, f.get_mut(..n).unwrap_or_default())?;
                let u16le = |i: usize| u16::from_le_bytes(at_n(&f, i));
                let mut tag = u16le(0);
                if tag == 0xFFFE && n >= 26 {
                    tag = u16le(24);
                }
                format = Some((tag, u16le(2), u16le(14)));
            }
            Some(b"data") => {
                let (tag, channels, bits) =
                    format.ok_or_else(|| invalid("WAV data before its format"))?;
                let float = match tag {
                    1 => false,
                    3 => true,
                    _ => return Err(invalid(&format!("WAV format tag {tag}"))),
                };
                return pcm(
                    pos + 8,
                    len.min(size - pos - 8),
                    channels,
                    bits,
                    float,
                    false,
                );
            }
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    Err(invalid("WAV without data"))
}

fn aiff(
    size: u64,
    aifc: bool,
    read: impl Fn(u64, &mut [u8]) -> io::Result<()>,
) -> io::Result<Layout> {
    let mut pos = 12;
    let mut comm = None;
    while pos + 8 <= size {
        let mut head = [0; 8];
        read(pos, &mut head)?;
        let len = u64::from(u32::from_be_bytes(at_n(&head, 4)));
        match head.get(..4) {
            Some(b"COMM") => {
                let mut c = [0; 22];
                read(
                    pos + 8,
                    c.get_mut(..if aifc { 22 } else { 18 }).unwrap_or_default(),
                )?;
                let channels = u16::from_be_bytes(at_n(&c, 0));
                let bits = u16::from_be_bytes(at_n(&c, 6));
                let (float, big_endian) = match c.get(18..22).unwrap_or_default() {
                    _ if !aifc => (false, true),
                    b"NONE" | b"twos" => (false, true),
                    b"sowt" => (false, false),
                    b"fl32" | b"FL32" => (true, true),
                    kind => {
                        let kind = String::from_utf8_lossy(kind);
                        return Err(unsupported(&format!("AIFC {kind}")));
                    }
                };
                comm = Some((channels, bits, float, big_endian));
            }
            Some(b"SSND") => {
                let (channels, bits, float, big_endian) =
                    comm.ok_or_else(|| invalid("AIFF sound before its format"))?;
                let mut o = [0; 4];
                read(pos + 8, &mut o)?;
                let start = pos + 16 + u64::from(u32::from_be_bytes(o));
                let len = (pos + 8 + len).min(size).saturating_sub(start);
                return pcm(start, len, channels, bits, float, big_endian);
            }
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    Err(invalid("AIFF without sound data"))
}

fn pcm(
    offset: u64,
    len: u64,
    channels: u16,
    bits: u16,
    float: bool,
    big_endian: bool,
) -> io::Result<Layout> {
    let channels =
        u8::try_from(channels).map_err(|_| unsupported(&format!("{channels} channels")))?;
    let bits = u8::try_from(bits).map_err(|_| unsupported(&format!("{bits}-bit")))?;
    Ok(Layout::Pcm {
        offset,
        len,
        channels,
        pcm: Pcm {
            bits,
            float,
            big_endian,
        },
    })
}

/// `N` bytes of `b` from `at`, zero where `b` is short.
fn at_n<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    b.get(at..at + N)
        .and_then(|s| s.try_into().ok())
        .unwrap_or([0; N])
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

fn unsupported(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, what)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(bytes: &[u8]) -> io::Result<Layout> {
        sniff(bytes.len() as u64, |pos, out| {
            out.copy_from_slice(&bytes[pos as usize..pos as usize + out.len()]);
            Ok(())
        })
    }

    #[test]
    fn wav_and_aiff_name_their_data_after_other_chunks() {
        let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
        wav.extend(b"LIST\x03\0\0\0abc\0");
        wav.extend(b"fmt \x10\0\0\0");
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(44_100u32.to_le_bytes());
        wav.extend([0; 6]);
        wav.extend(24u16.to_le_bytes());
        wav.extend(b"data\x0c\0\0\0");
        wav.extend([0; 12]);
        let pcm = Pcm {
            bits: 24,
            float: false,
            big_endian: false,
        };
        let layout = Layout::Pcm {
            offset: 56,
            len: 12,
            channels: 2,
            pcm,
        };
        assert_eq!(of(&wav).unwrap(), layout);

        let mut aiff = b"FORM\0\0\0\0AIFF".to_vec();
        aiff.extend(b"COMM\0\0\0\x12");
        aiff.extend(1u16.to_be_bytes());
        aiff.extend(2u32.to_be_bytes());
        aiff.extend(16u16.to_be_bytes());
        aiff.extend([0; 10]);
        aiff.extend(b"SSND\0\0\0\x0c");
        aiff.extend([0; 8]);
        aiff.extend([1, 2, 3, 4]);
        let pcm = Pcm {
            bits: 16,
            float: false,
            big_endian: true,
        };
        let layout = Layout::Pcm {
            offset: 54,
            len: 4,
            channels: 1,
            pcm,
        };
        assert_eq!(of(&aiff).unwrap(), layout);
    }
}
