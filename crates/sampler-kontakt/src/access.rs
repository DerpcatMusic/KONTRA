//! Library access, ported from the v1 importer (`src/access.rs`): reads a
//! library's own access fields from its `.nicnt` and builds the keystream for
//! its encrypted presets and archive members. No keys are embedded, logged or
//! persisted; the key lives only in the returned keystream.

use ni_file::nis::LibraryKey;
use std::{fs::File, io::Read, path::Path, sync::Arc};

/// The keystream of the library `path` belongs to, or why none was found.
/// The search stops at the library root (the folder holding `Samples`).
pub fn library_key(path: &Path) -> Result<Arc<dyn LibraryKey>, String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut checked = Vec::new();
    for parent in path.ancestors().skip(1) {
        let mut files: Vec<_> = std::fs::read_dir(parent)
            .map_err(|e| format!("reading {}: {e}", parent.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|file| file.extension().is_some_and(|e| e.eq_ignore_ascii_case("nicnt")))
            .collect();
        files.sort();
        for file in files {
            // Product XML precedes artwork. Bound reads even for malformed containers.
            let mut bytes = Vec::new();
            File::open(&file)
                .and_then(|f| f.take(64 * 1024).read_to_end(&mut bytes))
                .map_err(|e| format!("reading {}: {e}", file.display()))?;
            if let (Some(key), Some(iv)) = (field::<32>(&bytes, b"<JDX>"), field::<16>(&bytes, b"<HU>")) {
                return Ok(Arc::new(Keystream::new(key, iv)));
            }
            checked.push(format!("{}: no access fields in its first 64 KiB", file.display()));
        }
        if parent.join("Samples").is_dir() {
            break;
        }
    }
    Err(if checked.is_empty() {
        format!("no .nicnt access data found for {}", path.display())
    } else {
        checked.join("; ")
    })
}

/// Exactly `N` hex-encoded bytes inside the matching XML element.
fn field<const N: usize>(bytes: &[u8], tag: &[u8]) -> Option<[u8; N]> {
    let start = bytes.windows(tag.len()).position(|w| w == tag)? + tag.len();
    let closing = [b"</".as_slice(), tag.get(1..)?].concat();
    let end = bytes.get(start..)?.windows(closing.len()).position(|w| w == closing)? + start;
    let text = bytes.get(start..end)?.trim_ascii();
    if text.len() != N * 2 {
        return None;
    }
    let mut out = [0; N];
    for (dest, pair) in out.iter_mut().zip(text.chunks_exact(2)) {
        *dest = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// Library-provided AES-256 key and counter, expanded once into the
/// resource-relative stream that repeats every 64 KiB. Deliberately has no
/// Debug implementation.
struct Keystream {
    stream: Box<[u8; 65536]>,
}

impl Keystream {
    fn new(key: [u8; 32], iv: [u8; 16]) -> Self {
        use aes::cipher::{BlockEncrypt, KeyInit};
        let cipher = aes::Aes256::new((&key).into());
        let mut counter = u128::from_be_bytes(iv);
        let mut stream = Box::new([0u8; 65536]);
        let mut seed = 0x608da0a2u32;
        for chunk in stream.chunks_exact_mut(16) {
            let mut block = counter.to_be_bytes().into();
            cipher.encrypt_block(&mut block);
            for (out, value) in chunk.iter_mut().zip(block) {
                seed = seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
                *out = value ^ (seed >> 16) as u8;
            }
            counter = counter.wrapping_add(1);
        }
        Self { stream }
    }
}

impl LibraryKey for Keystream {
    /// The stream is position-relative, so members can be read at random offsets.
    fn apply_at(&self, offset: u64, bytes: &mut [u8]) {
        let len = self.stream.len();
        let mut at = (offset % len as u64) as usize;
        for run in bytes.chunks_mut(len) {
            let (head, tail) = run.split_at_mut((len - at).min(run.len()));
            for (byte, key) in head.iter_mut().zip(&self.stream[at..]) {
                *byte ^= key;
            }
            for (byte, key) in tail.iter_mut().zip(&self.stream[..]) {
                *byte ^= key;
            }
            at = (at + run.len()) % len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_cipher_vector_matches_v1() {
        let key = Keystream::new([0; 32], [0; 16]);
        let mut bytes = vec![0; 65568];
        key.apply(&mut bytes);
        assert_eq!(
            &bytes[..8],
            &[0xd2, 0x49, 0xf2, 0x9d, 0x7b, 0x04, 0x69, 0xed]
        );
        assert_eq!(&bytes[..32], &bytes[65536..]);
        key.apply(&mut bytes);
        assert!(bytes.iter().all(|b| *b == 0));
    }

    #[test]
    fn access_fields_need_their_closing_tag() {
        let hex = "ab".repeat(16);
        assert_eq!(field::<16>(format!("<HU> \n{hex}\n </HU>").as_bytes(), b"<HU>"), Some([0xab; 16]));
        assert_eq!(field::<16>(format!("<HU>{hex}</JDX>").as_bytes(), b"<HU>"), None);
        assert_eq!(field::<16>(format!("<HU>{hex}ab</HU>").as_bytes(), b"<HU>"), None);
    }
}
