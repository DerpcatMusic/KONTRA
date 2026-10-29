//! Optional stock artwork from a user's installed Kontakt executable.
//!
//! The four PNGs are identified by dimensions, byte length and CRC-32 from
//! Kontakt 8.13.1. Unknown versions safely use the built-in vector fallback;
//! no vendor artwork is distributed with KONTRA.

use kontra_ui_model::{ImageMeta, Orientation};
use mui::prelude::Image;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Bound background scanning and hashing; larger executables use the fallback.
const MAX_EXECUTABLE: u64 = 512 << 20;
// Bump the disk cache version whenever these fingerprints change.
const KEYS: [(&str, usize, u32, u32, u32); 5] = [
    ("@stock/base", 2638, 87, 39, 0x0841_8e0a),
    ("@stock/cap", 12945, 3200, 29, 0x5838_2c55),
    ("@stock/light", 37604, 4992, 39, 0x16d9_4b51),
    ("@stock/font", 2363, 1402, 13, 0xc762_083f),
    ("@stock/font13", 6034, 699, 13, 0xe3d1_56e4),
];

static ART: OnceLock<Option<super::cache::Art>> = OnceLock::new();

/// Start discovery, hashing and decoding before the editor's first frame.
pub(crate) fn prewarm() {
    static START: OnceLock<()> = OnceLock::new();
    START.get_or_init(|| {
        std::thread::spawn(|| {
            let _ = ART.get_or_init(load);
        });
    });
}

pub(super) fn frame(path: &str, meta: ImageMeta, n: u32) -> Option<Image> {
    let i = KEYS.iter().position(|(key, ..)| *key == path)?;
    let art = ART.get_or_init(load).as_ref()?;
    let image = if i == 4 {
        art.font13.as_ref()?
    } else {
        art.main.get(i)?
    };
    let frames = meta.frames.max(1);
    let (x, y, width, height) = match meta.axis {
        Orientation::Horizontal => (
            n.checked_mul(image.width / frames)?,
            0,
            image.width / frames,
            image.height,
        ),
        Orientation::Vertical => (
            0,
            n.checked_mul(image.height / frames)?,
            image.width,
            image.height / frames,
        ),
    };
    if width == 0
        || height == 0
        || x.checked_add(width)? > image.width
        || y.checked_add(height)? > image.height
    {
        return None;
    }
    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for row in y..y + height {
        let start = ((row as usize) * (image.width as usize) + x as usize) * 4;
        pixels.extend_from_slice(image.rgba.get(start..start + (width as usize) * 4)?);
    }
    Image::rgba(width, height, pixels)
}

fn load() -> Option<super::cache::Art> {
    super::discovery::candidates()
        .into_iter()
        .find_map(|path| load_from(&path))
}

fn load_from(path: &Path) -> Option<super::cache::Art> {
    let before = path.metadata().ok()?;
    if !before.is_file() || before.len() > MAX_EXECUTABLE {
        return None;
    }
    let dims = KEYS.map(|(_, _, w, h, _)| (w, h));
    match super::cache::load(path, &dims) {
        Some(super::cache::Cached::Images(images)) => return Some(*images),
        Some(super::cache::Cached::Missing) => return None,
        None => {}
    }
    let mut file = std::fs::File::open(path).ok()?;
    let mut found: [Option<Vec<u8>>; 5] = [None, None, None, None, None];
    let mut hash = Sha256::new();
    // Keep an overlap large enough for the longest candidate PNG.
    let overlap = 37604;
    let mut data = vec![0; 1 << 20];
    let mut carry = Vec::new();
    loop {
        let n = file.read(&mut data).ok()?;
        if n == 0 {
            break;
        }
        let bytes = data.get(..n)?;
        hash.update(bytes);
        // Continue hashing the complete executable after finding the artwork.
        if found.iter().all(Option::is_some) {
            continue;
        }
        carry.extend_from_slice(bytes);
        for (at, sig) in carry.windows(SIGNATURE.len()).enumerate() {
            if sig != SIGNATURE {
                continue;
            }
            for (i, (_, len, width, height, checksum)) in KEYS.iter().enumerate() {
                if found.get(i)?.is_some() {
                    continue;
                }
                let Some(png) = carry.get(at..at.checked_add(*len)?) else {
                    continue;
                };
                if png.get(12..16) != Some(b"IHDR")
                    || png.get(16..20) != Some(width.to_be_bytes().as_slice())
                    || png.get(20..24) != Some(height.to_be_bytes().as_slice())
                    || crc32(png) != *checksum
                {
                    continue;
                }
                *found.get_mut(i)? = Some(png.to_vec());
            }
        }
        let keep = carry.len().saturating_sub(overlap);
        carry.drain(..keep);
    }
    let after = path.metadata().ok()?;
    if before.len() != after.len() || before.modified().ok()? != after.modified().ok()? {
        return None;
    }
    if found.get(..4)?.iter().any(Option::is_none) {
        super::cache::missing(path);
        return None;
    }
    let [base, cap, light, font, font13] = found;
    let decode = |png: Vec<u8>| super::decode(&png, ImageMeta::default(), 0);
    let images = super::cache::Art {
        main: [
            decode(base?)?,
            decode(cap?)?,
            decode(light?)?,
            decode(font?)?,
        ],
        font13: font13.and_then(decode),
    };
    super::cache::save(path, &format!("{:x}", hash.finalize()), &images);
    Some(images)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc32_matches_the_standard_check_vector() {
        assert_eq!(super::crc32(b"123456789"), 0xcbf4_3926);
    }
}
