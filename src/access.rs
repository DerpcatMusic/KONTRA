//! Library access: reads a library's own access fields from its `.nicnt`
//! and builds the keystream for its encrypted presets and archive members.
//! Built only with the `library-access` feature; `no_access.rs` stands in
//! otherwise. No keys are embedded, logged or persisted.

use anyhow::{Context, Result};
use ni_file::nis::LibraryKey;
use std::{fs::File, io::Read, path::Path, sync::Arc};

/// The access key of the library `path` belongs to, if locally available.
pub(crate) fn library_key(path: &Path) -> Result<Option<Arc<dyn LibraryKey>>> {
    Ok(lookup(path)?.0)
}

/// Required by encrypted content; report why local access data was not found.
pub(crate) fn require_library_key(path: &Path) -> Result<Arc<dyn LibraryKey>> {
    let (key, reason) = lookup(path)?;
    key.context(reason)
}

fn lookup(path: &Path) -> Result<(Option<Arc<dyn LibraryKey>>, String)> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut checked = Vec::new();
    let mut last = path.clone();
    for parent in path.ancestors().skip(1) {
        last = parent.to_owned();
        let mut files = std::fs::read_dir(parent)
            .with_context(|| {
                format!(
                    "Reading local library access directory {}",
                    parent.display()
                )
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        files.sort();
        for file in files {
            if !file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
            {
                continue;
            }
            // Product XML precedes artwork. Bound reads even for malformed containers.
            let mut bytes = Vec::new();
            File::open(&file)
                .with_context(|| {
                    format!("Reading local library access metadata {}", file.display())
                })?
                .take(64 * 1024)
                .read_to_end(&mut bytes)?;
            let key = field::<32>(&bytes, b"<JDX>");
            let iv = field::<16>(&bytes, b"<HU>");
            if let (Some(key), Some(iv)) = (key, iv) {
                return Ok((Some(Arc::new(Keystream::new(key, iv))), String::new()));
            }
            checked.push(format!(
                "{}: missing or malformed {}{} in first 64 KiB",
                file.display(),
                if key.is_none() { "JDX" } else { "" },
                if iv.is_none() {
                    if key.is_none() {
                        "/HU"
                    } else {
                        "HU"
                    }
                } else {
                    ""
                }
            ));
        }
        if parent.join("Samples").is_dir() {
            break;
        }
    }
    let reason = if checked.is_empty() {
        format!(
            "no .nicnt metadata found between {} and {}",
            path.parent().unwrap_or(&path).display(),
            last.display()
        )
    } else {
        checked.join("; ")
    };
    Ok((
        None,
        format!(
            "Encrypted library content needs local access data for {}: {reason}",
            path.display()
        ),
    ))
}

/// Exactly `N` hex-encoded bytes inside the matching XML element.
fn field<const N: usize>(bytes: &[u8], tag: &[u8]) -> Option<[u8; N]> {
    let start = bytes.windows(tag.len()).position(|w| w == tag)? + tag.len();
    let closing = [b"</".as_slice(), tag.get(1..)?].concat();
    let end = bytes
        .get(start..)?
        .windows(closing.len())
        .position(|w| w == closing)?
        + start;
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
            field::<16>(format!("<HU> \r\n{hex}\n </HU>").as_bytes(), b"<HU>"),
            Some([0xab; 16])
        );
        assert_eq!(
            field::<16>(format!("<HU>{hex}</JDX>").as_bytes(), b"<HU>"),
            None
        );
        assert_eq!(
            field::<16>(format!("<HU>{hex}</HU>").as_bytes(), b"<HU>"),
            Some([0xab; 16])
        );
        assert_eq!(
            field::<16>(format!("<HU>{hex}ab</HU>").as_bytes(), b"<HU>"),
            None
        );
    }

    #[test]
    fn local_metadata_lookup_decrypts_and_explains_missing_fields() {
        let root = std::env::temp_dir().join(format!("kontra-access-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        std::fs::create_dir_all(root.join("Samples")).unwrap();
        let preset = root.join("Instruments/authored.nki");
        assert!(library_key(&preset).unwrap().is_none());
        let reason = require_library_key(&preset).err().unwrap().to_string();
        assert!(reason.contains("no .nicnt metadata found"), "{reason}");
        let metadata = root.join("authored.NICNT");
        std::fs::write(&metadata, "<JDX>00</JDX><HU>00</HU>").unwrap();
        let reason = require_library_key(&preset).err().unwrap().to_string();
        assert!(reason.contains("missing or malformed JDX/HU"), "{reason}");
        // All-zero authored access data, not a native library key.
        std::fs::write(
            &metadata,
            format!(
                "<JDX>\n {} \r\n</JDX><HU> {} </HU>",
                "00".repeat(32),
                "00".repeat(16)
            ),
        )
        .unwrap();
        let key = require_library_key(&preset).unwrap();
        let mut payload = vec![3, b't', b'e', b's', b't'];
        Keystream::new([0; 32], [0; 16]).apply(&mut payload);
        let mut frame = 1u32.to_le_bytes().to_vec();
        frame.push(1);
        frame.extend(4u32.to_le_bytes());
        frame.extend(5u32.to_le_bytes());
        frame.extend(payload);
        assert_eq!(
            SubtreeItem::read_with_key(Cursor::new(&frame), Some(&*key))
                .unwrap()
                .inner_data,
            b"test"
        );
        std::fs::write(
            &metadata,
            format!("<JDX>{}</JDX><HU>{}</HU>", "01".repeat(32), "00".repeat(16)),
        )
        .unwrap();
        let wrong = require_library_key(&preset).unwrap();
        assert!(SubtreeItem::read_with_key(Cursor::new(&frame), Some(&*wrong)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
