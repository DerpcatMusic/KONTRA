//! Library access: the keystream a library's own `.nicnt` describes, for its
//! encrypted presets and archive members. Keys are never logged or kept.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use ni_file::nis::LibraryKey;

/// The keystream of the library `path` belongs to, for presets.
pub(crate) fn library_key(path: &Path) -> Result<Arc<dyn LibraryKey>, String> {
    Ok(Arc::new(Key(keystream(path)?)))
}

/// The 64 KiB keystream of the library `path` belongs to; the search stops at
/// the library root (the folder holding `Samples`).
pub(crate) fn keystream(path: &Path) -> Result<Arc<[u8]>, String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    for parent in path.ancestors().skip(1) {
        let mut files: Vec<_> = std::fs::read_dir(parent)
            .map_err(|e| format!("reading {}: {e}", parent.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|f| {
                f.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
            })
            .collect();
        files.sort();
        for file in files {
            // Product XML precedes artwork.
            let mut bytes = Vec::new();
            File::open(&file)
                .and_then(|f| f.take(64 * 1024).read_to_end(&mut bytes))
                .map_err(|e| format!("reading {}: {e}", file.display()))?;
            if let (Some(key), Some(iv)) = (field::<32>(&bytes, b"JDX"), field::<16>(&bytes, b"HU"))
            {
                return Ok(expand(key, iv));
            }
        }
        if parent.join("Samples").is_dir() {
            break;
        }
    }
    Err(format!("no .nicnt access data for {}", path.display()))
}

/// Exactly `N` hex-encoded bytes inside `<tag>…</tag>`.
fn field<const N: usize>(bytes: &[u8], tag: &[u8]) -> Option<[u8; N]> {
    let open = [b"<", tag, b">"].concat();
    let close = [b"</", tag, b">"].concat();
    let start = bytes.windows(open.len()).position(|w| w == open)? + open.len();
    let rest = bytes.get(start..)?;
    let end = rest.windows(close.len()).position(|w| w == close)?;
    let text = rest.get(..end)?.trim_ascii();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0; N];
    for (dest, pair) in out.iter_mut().zip(text.as_chunks::<2>().0) {
        *dest = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// AES-256 counter stream mixed with an LCG, expanded once; it repeats every 64 KiB.
fn expand(key: [u8; 32], iv: [u8; 16]) -> Arc<[u8]> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    let cipher = aes::Aes256::new((&key).into());
    let mut counter = u128::from_be_bytes(iv);
    let mut stream = vec![0u8; 65_536];
    let mut seed = 0x608d_a0a2_u32;
    for chunk in stream.as_chunks_mut::<16>().0 {
        let mut block = counter.to_be_bytes().into();
        cipher.encrypt_block(&mut block);
        for (out, value) in chunk.iter_mut().zip(block) {
            seed = seed.wrapping_mul(0x343fd).wrapping_add(0x0026_9ec3);
            *out = value ^ (seed >> 16) as u8;
        }
        counter = counter.wrapping_add(1);
    }
    stream.into()
}

/// A keystream as ni-file applies it. Deliberately not `Debug`.
struct Key(Arc<[u8]>);

impl LibraryKey for Key {
    fn apply_at(&self, offset: u64, bytes: &mut [u8]) {
        let key = kontra_ir::Keystream::Repeat {
            stream: Arc::clone(&self.0),
            origin: 0,
        };
        key.apply(offset, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keystream_matches_v1_and_repeats_every_64_kib() {
        let key = Key(expand([0; 32], [0; 16]));
        let mut bytes = vec![0; 65_568];
        key.apply(&mut bytes);
        assert_eq!(
            &bytes[..8],
            &[0xd2, 0x49, 0xf2, 0x9d, 0x7b, 0x04, 0x69, 0xed]
        );
        assert_eq!(&bytes[..32], &bytes[65_536..]);
    }

    #[test]
    fn a_field_needs_its_own_closing_tag_and_exact_length() {
        let hex = "ab".repeat(16);
        assert_eq!(
            field::<16>(format!("<HU> {hex}\n</HU>").as_bytes(), b"HU"),
            Some([0xab; 16])
        );
        assert_eq!(
            field::<16>(format!("<HU>{hex}</JDX>").as_bytes(), b"HU"),
            None
        );
        assert_eq!(
            field::<16>(format!("<HU>{hex}ab</HU>").as_bytes(), b"HU"),
            None
        );
    }
}
