//! Reader for Ample Sound `.ascf` sample containers (`aswv` blocks).
//!
//! A file is a run of blocks with no index and no trailing data. Each block is
//! a 32-byte header followed by `frames * channels * bytes_per_sample` bytes of
//! interleaved little-endian signed PCM. The layout is documented in
//! `docs/re/formats/ascf.md`; every field below was checked against the
//! installed libraries (482 files, all chaining exactly to end of file).
//!
//! Only the container and PCM are read here. Which block plays on which key
//! and velocity is not in the container; see the docs for where it lives.

use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom},
};

/// `aswv` magic at the start of every block.
pub const MAGIC: &[u8; 4] = b"aswv";
/// Constant marker after the magic, present in every block.
pub const SIGNATURE: &[u8; 14] = b"PleaseDontHack";
/// Bytes of header preceding each block's PCM.
pub const HEADER_LEN: u64 = 32;

/// One block: a header and where its PCM lives in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// Byte offset of the block's `aswv` magic.
    pub offset: u64,
    /// 1 (mono) or 2 (stereo) in every observed file.
    pub channels: u8,
    /// Always 3 (24-bit) in every observed file.
    pub bytes_per_sample: u8,
    /// Sample frames (one frame = one sample per channel).
    pub frames: u32,
    /// Four-byte tag at header offset 28. Observed as `boom` in every block.
    pub tag: [u8; 4],
}

impl Block {
    /// Byte offset of the first PCM byte.
    pub fn data_offset(&self) -> u64 {
        self.offset + HEADER_LEN
    }

    /// Length of the PCM payload in bytes.
    pub fn data_len(&self) -> u64 {
        u64::from(self.frames) * u64::from(self.channels) * u64::from(self.bytes_per_sample)
    }

    /// Byte offset just past this block; the next block starts here.
    pub fn end(&self) -> u64 {
        self.data_offset() + self.data_len()
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// Bytes at `offset` do not start a block (no `aswv` + signature).
    BadBlock {
        offset: u64,
    },
    /// A block at `offset` declares a sample layout this reader does not decode.
    UnsupportedLayout {
        offset: u64,
        channels: u8,
        bytes_per_sample: u8,
    },
    /// A block at `offset` claims PCM past the end of the file.
    Truncated {
        offset: u64,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "ascf: {e}"),
            Error::BadBlock { offset } => write!(f, "ascf: no aswv block at byte {offset}"),
            Error::UnsupportedLayout {
                offset,
                channels,
                bytes_per_sample,
            } => write!(
                f,
                "ascf: block at {offset} has {channels} channel(s) x {bytes_per_sample} byte(s), only 1-2 x 3 is known"
            ),
            Error::Truncated { offset } => {
                write!(f, "ascf: block at {offset} runs past end of file")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// Lists every block in the file, checking that they tile it exactly.
pub fn scan<R: Read + Seek>(reader: &mut R) -> Result<Vec<Block>, Error> {
    let size = reader.seek(SeekFrom::End(0))?;
    let mut blocks = Vec::new();
    let mut offset = 0;
    while offset < size {
        let block = read_header(reader, offset)?;
        if block.end() > size {
            return Err(Error::Truncated { offset });
        }
        offset = block.end();
        blocks.push(block);
    }
    Ok(blocks)
}

fn read_header<R: Read + Seek>(reader: &mut R, offset: u64) -> Result<Block, Error> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut h = [0u8; HEADER_LEN as usize];
    reader.read_exact(&mut h).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => Error::Truncated { offset },
        _ => Error::Io(e),
    })?;
    if &h[0..4] != MAGIC || &h[4..18] != SIGNATURE {
        return Err(Error::BadBlock { offset });
    }
    let channels = h[18];
    let bytes_per_sample = h[19];
    if !matches!(channels, 1 | 2) || bytes_per_sample != 3 {
        return Err(Error::UnsupportedLayout {
            offset,
            channels,
            bytes_per_sample,
        });
    }
    Ok(Block {
        offset,
        channels,
        bytes_per_sample,
        frames: u32::from_le_bytes([h[20], h[21], h[22], h[23]]),
        tag: [h[28], h[29], h[30], h[31]],
    })
}

/// Decodes one block's PCM to stereo `f32` frames in [-1, 1), at the file's
/// own rate (not stored in the container). Mono is duplicated to both sides,
/// matching `sampler_kontakt::Decoded`.
pub fn decode<R: Read + Seek>(reader: &mut R, block: &Block) -> Result<Vec<[f32; 2]>, Error> {
    reader.seek(SeekFrom::Start(block.data_offset()))?;
    let mut raw = vec![0u8; block.data_len() as usize];
    reader.read_exact(&mut raw)?;
    const SCALE: f32 = 1.0 / 8_388_608.0; // 2^23
    let sample = |b: &[u8]| {
        // Sign-extend the 24-bit little-endian value.
        let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
        v as f32 * SCALE
    };
    Ok(match block.channels {
        1 => raw
            .as_chunks::<3>()
            .0
            .iter()
            .map(|b| {
                let s = sample(b);
                [s, s]
            })
            .collect(),
        _ => raw
            .as_chunks::<6>()
            .0
            .iter()
            .map(|b| [sample(&b[0..3]), sample(&b[3..6])])
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn block_bytes(channels: u8, pcm: &[u8]) -> Vec<u8> {
        let frames = pcm.len() as u32 / (u32::from(channels) * 3);
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(SIGNATURE);
        v.extend_from_slice(&[channels, 3]);
        v.extend_from_slice(&frames.to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(b"boom");
        v.extend_from_slice(pcm);
        v
    }

    #[test]
    fn synthetic_stereo_and_mono_blocks_tile_and_decode() {
        // Stereo frame 0 = (full-scale negative, 0), frame 1 = (+max, -1 LSB).
        let stereo = [0, 0, 0x80, 0, 0, 0, 0xff, 0xff, 0x7f, 0xff, 0xff, 0xff];
        // Mono: one sample at 0x010000 = 2^16 / 2^23 = 1/128.
        let mono = [0, 0, 1];
        let mut file = block_bytes(2, &stereo);
        file.extend(block_bytes(1, &mono));
        let mut cur = Cursor::new(file);
        let blocks = scan(&mut cur).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].frames, 2);
        assert_eq!(blocks[0].tag, *b"boom");
        assert_eq!(blocks[1].offset, blocks[0].end());
        let st = decode(&mut cur, &blocks[0]).unwrap();
        assert_eq!(st[0], [-1.0, 0.0]);
        assert!((st[1][0] - (8_388_607.0 / 8_388_608.0)).abs() < 1e-7);
        assert_eq!(st[1][1], -1.0 / 8_388_608.0);
        let mo = decode(&mut cur, &blocks[1]).unwrap();
        assert_eq!(mo, [[1.0 / 128.0, 1.0 / 128.0]]);
    }

    #[test]
    fn rejects_bad_magic_truncation_and_unknown_layout() {
        let mut bad = block_bytes(2, &[0; 6]);
        bad[0] = b'x';
        assert!(matches!(
            scan(&mut Cursor::new(bad)),
            Err(Error::BadBlock { offset: 0 })
        ));

        let mut short = block_bytes(2, &[0; 6]);
        short.truncate(short.len() - 1);
        assert!(matches!(
            scan(&mut Cursor::new(short)),
            Err(Error::Truncated { offset: 0 })
        ));

        let mut wide = block_bytes(2, &[0; 6]);
        wide[19] = 2;
        assert!(matches!(
            scan(&mut Cursor::new(wide)),
            Err(Error::UnsupportedLayout {
                channels: 2,
                bytes_per_sample: 2,
                ..
            })
        ));
    }

    /// Every installed Ample file must tile exactly. Skipped where the library is absent.
    #[test]
    fn installed_libraries_all_load() {
        let root = std::path::Path::new("/data/main/Libraries/Ample");
        let Ok(dirs) = std::fs::read_dir(root) else {
            return;
        };
        let mut files = 0usize;
        let mut blocks = 0usize;
        for dir in dirs.flatten() {
            let Ok(entries) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "ascf") {
                    continue;
                }
                let mut f = std::fs::File::open(&path).unwrap();
                let list = scan(&mut f).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(!list.is_empty(), "{}", path.display());
                assert!(list.iter().all(|b| b.tag == *b"boom"), "{}", path.display());
                files += 1;
                blocks += list.len();
            }
        }
        eprintln!("ascf: {files} files, {blocks} blocks scanned");
        if files > 0 {
            assert_eq!(files, 482);
        }
    }

    #[test]
    fn first_block_of_agmcm1_decodes_to_recorded_samples() {
        let path = std::path::Path::new("/data/main/Libraries/Ample/AGM_Library/AGMCm-1.ascf");
        let Ok(mut f) = std::fs::File::open(path) else {
            return;
        };
        let blocks = scan(&mut f).unwrap();
        let b = blocks[0];
        assert_eq!((b.channels, b.frames), (2, 22050));
        let frames = decode(&mut f, &b).unwrap();
        // Frame 0 is silent; frame 1 is L=-79, R=238 LSBs (24-bit, little-endian).
        assert_eq!(frames[0], [0.0, 0.0]);
        assert_eq!(frames[1], [-79.0 / 8_388_608.0, 238.0 / 8_388_608.0]);
    }
}
