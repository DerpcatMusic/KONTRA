//! Library access: reads a library's own access fields from its `.nicnt`
//! and builds the keystream for its encrypted presets and archive members.
//! Built only with the `library-access` feature; `no_access.rs` stands in
//! otherwise. No keys are embedded, logged or persisted.

use anyhow::Result;
use ni_file::nis::LibraryKey;
use std::{fs::File, io::Read, path::Path, sync::Arc};

/// The access key of the library `path` belongs to: the first `.nicnt` in
/// its folders (up to the library root) with access fields, if any.
pub(crate) fn library_key(path: &Path) -> Result<Option<Arc<dyn LibraryKey>>> {
    for parent in path.ancestors().skip(1) {
        for entry in std::fs::read_dir(parent)? {
            let file = entry?.path();
            if !file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
            {
                continue;
            }
            // Product XML precedes artwork. Bound reads even for malformed containers.
            let mut bytes = Vec::new();
            File::open(&file)?.take(64 * 1024).read_to_end(&mut bytes)?;
            if let (Some(key), Some(iv)) =
                (field::<32>(&bytes, b"<JDX>"), field::<16>(&bytes, b"<HU>"))
            {
                return Ok(Some(Arc::new(Keystream::new(key, iv))));
            }
        }
        if parent.join("Samples").is_dir() {
            break;
        }
    }
    Ok(None)
}

/// `N` hex-encoded bytes following `tag`, closed by the next tag.
fn field<const N: usize>(bytes: &[u8], tag: &[u8]) -> Option<[u8; N]> {
    let start = bytes.windows(tag.len()).position(|w| w == tag)? + tag.len();
    let text = bytes.get(start..start + N * 2)?;
    let mut out = [0; N];
    for (dest, pair) in out.iter_mut().zip(text.chunks_exact(2)) {
        *dest = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    (bytes.get(start + N * 2) == Some(&b'<')).then_some(out)
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
        // Whole runs up to the stream's wrap-around: a plain XOR that vectorizes.
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
    use ni_file::nis::SubtreeItem;
    use std::io::Cursor;

    #[test]
    fn resource_cipher_vector() {
        let key = Keystream::new([0; 32], [0; 16]);
        let mut bytes = vec![0; 65568];
        key.apply(&mut bytes);
        assert_eq!(
            &bytes[..32],
            &[
                0xd2, 0x49, 0xf2, 0x9d, 0x7b, 0x04, 0x69, 0xed, 0x56, 0x08, 0xaa, 0xcc, 0x8f, 0x26,
                0xe2, 0xce, 0x60, 0x69, 0x27, 0xdf, 0xce, 0xaa, 0x1d, 0x20, 0xb7, 0xf7, 0x7a, 0xb0,
                0x72, 0x90, 0x7e, 0x8f
            ]
        );
        assert_eq!(&bytes[..32], &bytes[65536..]);
        key.apply(&mut bytes);
        assert!(bytes.iter().all(|b| *b == 0));
    }

    #[test]
    fn encrypted_subtree_requires_the_matching_key() {
        let key = Keystream::new([0; 32], [0; 16]);
        let mut payload = vec![3, b't', b'e', b's', b't'];
        key.apply(&mut payload);
        let mut frame = 1u32.to_le_bytes().to_vec();
        frame.push(1);
        frame.extend(4u32.to_le_bytes());
        frame.extend(5u32.to_le_bytes());
        frame.extend(payload);
        let read = |k: &Keystream| {
            SubtreeItem::read_with_key(Cursor::new(&frame), Some(k as &dyn LibraryKey))
        };
        assert_eq!(read(&key).unwrap().inner_data, b"test");
        assert!(read(&Keystream::new([1; 32], [0; 16])).is_err());
    }

    #[test]
    fn key_stream_is_position_relative_across_wraps() {
        let key = Keystream::new([3; 32], [5; 16]);
        let plain: Vec<u8> = (0..300_000u32).map(|i| (i * 31) as u8).collect();
        let mut whole = plain.clone();
        key.apply(&mut whole);
        // Any split decrypts to the same bytes, including runs across the 64 KiB wrap.
        for (at, len) in [
            (0, 1),
            (65_530, 20),
            (65_536, 65_536),
            (1_000, 200_000),
            (299_999, 1),
        ] {
            let mut part = whole[at..at + len].to_vec();
            key.apply_at(at as u64, &mut part);
            assert_eq!(part, &plain[at..at + len], "at {at}");
        }
    }

    #[test]
    fn access_fields_need_their_closing_tag() {
        let hex = "ab".repeat(16);
        assert_eq!(
            field::<16>(format!("<HU>{hex}</HU>").as_bytes(), b"<HU>"),
            Some([0xab; 16])
        );
        assert_eq!(
            field::<16>(format!("<HU>{hex}ab</HU>").as_bytes(), b"<HU>"),
            None
        );
    }
}
