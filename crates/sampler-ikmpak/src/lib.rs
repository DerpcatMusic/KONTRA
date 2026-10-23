//! IK Multimedia `IKMPAK` sample packs (Modo Drum `.pak`).
//!
//! Container layout, payload codec and naming grammar are in
//! `docs/re/formats/ikmpak.md`. This crate reads the table of contents and
//! hands out payload bytes. It never decodes audio or decrypts anything: the
//! payloads are plain RIFF/WAVE files.

pub mod member;

use std::io::{self, BufReader, Read, Seek, SeekFrom};

pub use member::{Member, Mic, note_number, parse_member};

const MAGIC: &[u8; 6] = b"IKMPAK";
const VERSION: u32 = 1;
/// Magic (6) + version (u32) + entry count (u32).
const HEADER_LEN: u64 = 14;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Magic,
    Version(u32),
    /// The entry at `index` is not a NUL-terminated path followed by two u32s inside the file.
    Entry {
        index: usize,
        reason: &'static str,
    },
    /// The entry index passed to a payload accessor is out of range.
    NoSuchEntry(usize),
    /// The payload is not a RIFF/WAVE file with a `fmt ` and `data` chunk.
    NotWav(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "ikmpak: {e}"),
            Error::Magic => f.write_str("ikmpak: not an IKMPAK file"),
            Error::Version(v) => write!(f, "ikmpak: unsupported version {v}"),
            Error::Entry { index, reason } => write!(f, "ikmpak: entry {index}: {reason}"),
            Error::NoSuchEntry(i) => write!(f, "ikmpak: no entry {i}"),
            Error::NotWav(why) => write!(f, "ikmpak: payload is not WAV: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// One table-of-contents entry: a path inside the pack and its payload range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub offset: u64,
    pub size: u64,
}

/// Format of a payload WAV, read from its `fmt ` and `data` chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavFormat {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub data_frames: u64,
}

/// An opened pack. Only the table of contents is read up front.
pub struct Pak<R> {
    reader: R,
    len: u64,
    entries: Vec<Entry>,
}

impl<R: Read + Seek> Pak<R> {
    pub fn open(mut reader: R) -> Result<Self, Error> {
        let len = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let mut head = [0u8; HEADER_LEN as usize];
        reader.read_exact(&mut head)?;
        if &head[..6] != MAGIC {
            return Err(Error::Magic);
        }
        let version = u32::from_le_bytes(head[6..10].try_into().unwrap());
        if version != VERSION {
            return Err(Error::Version(version));
        }
        let count = u32::from_le_bytes(head[10..14].try_into().unwrap()) as usize;

        let mut toc = BufReader::new(reader.by_ref());
        let mut entries = Vec::with_capacity(count);
        let mut pos = HEADER_LEN;
        for index in 0..count {
            let mut path = Vec::new();
            let read = io::BufRead::read_until(&mut toc, 0, &mut path)?;
            if read == 0 || path.last() != Some(&0) {
                return Err(Error::Entry {
                    index,
                    reason: "path is not NUL-terminated",
                });
            }
            path.pop();
            let path = String::from_utf8(path).map_err(|_| Error::Entry {
                index,
                reason: "path is not UTF-8",
            })?;
            let mut fields = [0u8; 8];
            toc.read_exact(&mut fields).map_err(|_| Error::Entry {
                index,
                reason: "entry ends inside the table of contents",
            })?;
            let offset = u32::from_le_bytes(fields[..4].try_into().unwrap()) as u64;
            let size = u32::from_le_bytes(fields[4..].try_into().unwrap()) as u64;
            pos += read as u64 + 8;
            if offset < pos {
                return Err(Error::Entry {
                    index,
                    reason: "payload starts inside the table of contents",
                });
            }
            if offset.checked_add(size).is_none_or(|end| end > len) {
                return Err(Error::Entry {
                    index,
                    reason: "payload runs past end of file",
                });
            }
            entries.push(Entry { path, offset, size });
        }
        Ok(Self {
            reader,
            len,
            entries,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Length of the pack file, as seen when it was opened.
    pub fn file_len(&self) -> u64 {
        self.len
    }

    /// The payload bytes of entry `index`, copied out of the pack.
    pub fn payload(&mut self, index: usize) -> Result<Vec<u8>, Error> {
        let e = self.entry(index)?.clone();
        self.reader.seek(SeekFrom::Start(e.offset))?;
        let mut buf = vec![0u8; e.size as usize];
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Reads the `fmt ` and `data` chunks of entry `index` without copying the audio.
    pub fn wav_format(&mut self, index: usize) -> Result<WavFormat, Error> {
        let e = self.entry(index)?.clone();
        let end = e.offset + e.size;
        let mut riff = [0u8; 12];
        self.reader.seek(SeekFrom::Start(e.offset))?;
        if e.size < 12 || self.reader.read_exact(&mut riff).is_err() {
            return Err(Error::NotWav("shorter than a RIFF header"));
        }
        if &riff[..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
            return Err(Error::NotWav("missing RIFF/WAVE"));
        }
        let mut pos = e.offset + 12;
        let mut fmt = None;
        let mut data_bytes = None;
        while pos + 8 <= end && data_bytes.is_none() {
            let mut ch = [0u8; 8];
            self.reader.seek(SeekFrom::Start(pos))?;
            self.reader.read_exact(&mut ch)?;
            let size = u32::from_le_bytes(ch[4..].try_into().unwrap()) as u64;
            match &ch[..4] {
                b"fmt " => {
                    let mut f = [0u8; 16];
                    self.reader.read_exact(&mut f)?;
                    fmt = Some(f);
                }
                b"data" => data_bytes = Some(size),
                _ => {}
            }
            pos += 8 + size + (size & 1);
        }
        let (Some(f), Some(data)) = (fmt, data_bytes) else {
            return Err(Error::NotWav("no fmt or data chunk"));
        };
        let format_tag = u16::from_le_bytes([f[0], f[1]]);
        let channels = u16::from_le_bytes([f[2], f[3]]);
        let sample_rate = u32::from_le_bytes(f[4..8].try_into().unwrap());
        let bits_per_sample = u16::from_le_bytes([f[14], f[15]]);
        let frame_bytes = channels as u64 * bits_per_sample.div_ceil(8) as u64;
        if frame_bytes == 0 {
            return Err(Error::NotWav("zero-width frame"));
        }
        Ok(WavFormat {
            format_tag,
            channels,
            sample_rate,
            bits_per_sample,
            data_frames: data / frame_bytes,
        })
    }

    fn entry(&self, index: usize) -> Result<&Entry, Error> {
        self.entries.get(index).ok_or(Error::NoSuchEntry(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Builds a pack in memory from `(path, payload)` pairs, laid out like the
    /// shipped packs: TOC first, payloads back to back.
    fn build(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut toc = Vec::new();
        for (p, _) in entries {
            toc.extend_from_slice(p.as_bytes());
            toc.push(0);
            toc.extend_from_slice(&[0u8; 8]);
        }
        let mut out = b"IKMPAK".to_vec();
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&toc);
        let mut at = out.len() as u32;
        let mut fields = Vec::new();
        for (_, data) in entries {
            fields.push((at, data.len() as u32));
            at += data.len() as u32;
        }
        // Rewrite the fixed fields now that offsets are known.
        let mut cursor = 14;
        for ((p, _), (off, size)) in entries.iter().zip(&fields) {
            cursor += p.len() + 1;
            out[cursor..cursor + 4].copy_from_slice(&off.to_le_bytes());
            out[cursor + 4..cursor + 8].copy_from_slice(&size.to_le_bytes());
            cursor += 8;
        }
        for (_, data) in entries {
            out.extend_from_slice(data);
        }
        out
    }

    fn wav(frames: u32) -> Vec<u8> {
        let data_len = frames * 4;
        let mut w = b"RIFF".to_vec();
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&48000u32.to_le_bytes());
        w.extend_from_slice(&(48000u32 * 4).to_le_bytes());
        w.extend_from_slice(&4u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        w.extend(std::iter::repeat_n(0u8, data_len as usize));
        w
    }

    #[test]
    fn synthetic_pack_round_trips() {
        let a = wav(10);
        let b = wav(3);
        let bytes = build(&[
            ("Kits/A/x_v127_rr1 Snare-DRY.wav", &a),
            ("Kits/A/x_v127_rr2 Snare-WET.wav", &b),
        ]);
        let mut pak = Pak::open(Cursor::new(bytes)).unwrap();
        assert_eq!(pak.entries().len(), 2);
        assert_eq!(pak.entries()[1].path, "Kits/A/x_v127_rr2 Snare-WET.wav");
        assert_eq!(pak.payload(0).unwrap(), a);
        assert_eq!(
            pak.wav_format(1).unwrap(),
            WavFormat {
                format_tag: 1,
                channels: 2,
                sample_rate: 48000,
                bits_per_sample: 16,
                data_frames: 3
            }
        );
        assert!(matches!(pak.payload(2), Err(Error::NoSuchEntry(2))));
    }

    #[test]
    fn rejects_bad_header_and_overrun() {
        assert!(matches!(
            Pak::open(Cursor::new(b"NOTPAK\x01\0\0\0\0\0\0\0".to_vec())),
            Err(Error::Magic)
        ));
        let mut bytes = build(&[("a.wav", &wav(1))]);
        let len = bytes.len();
        bytes.truncate(len - 1);
        assert!(matches!(
            Pak::open(Cursor::new(bytes)),
            Err(Error::Entry { index: 0, .. })
        ));
    }
}

mod format;
pub use format::FORMAT;
