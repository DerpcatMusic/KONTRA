//! Optional stock artwork from a user's installed Kontakt executable.
//!
//! The four PNGs are identified by dimensions, byte length and CRC-32 from
//! Kontakt 8.13.1. Unknown versions safely use the built-in vector fallback;
//! no vendor artwork is distributed with KONTRA.

use std::io::Read;
use std::sync::OnceLock;

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const KEYS: [(&str, usize, u32, u32, u32); 4] = [
    ("@stock/base", 2638, 87, 39, 0x0841_8e0a),
    ("@stock/cap", 12945, 3200, 29, 0x5838_2c55),
    ("@stock/light", 37604, 4992, 39, 0x16d9_4b51),
    ("@stock/font", 2363, 1402, 13, 0xc762_083f),
];

pub(super) fn read(path: &str) -> Option<Vec<u8>> {
    static ART: OnceLock<Option<[Vec<u8>; 4]>> = OnceLock::new();
    let i = KEYS.iter().position(|(key, ..)| *key == path)?;
    ART.get_or_init(load).as_ref()?.get(i).cloned()
}

fn load() -> Option<[Vec<u8>; 4]> {
    let path = std::env::var_os("KONTAKT_STOCK_EXECUTABLE")?;
    let mut file = std::fs::File::open(path).ok()?;
    let mut found: [Option<Vec<u8>>; 4] = [None, None, None, None];
    // Keep an overlap large enough for the longest candidate PNG.
    let overlap = 37604;
    let mut data = vec![0; 1 << 20];
    let mut carry = Vec::new();
    loop {
        let n = file.read(&mut data).ok()?;
        if n == 0 {
            break;
        }
        carry.extend_from_slice(data.get(..n)?);
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
        if found.iter().all(Option::is_some) {
            break;
        }
        let keep = carry.len().saturating_sub(overlap);
        carry.drain(..keep);
    }
    let [base, cap, light, font] = found;
    Some([base?, cap?, light?, font?])
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
