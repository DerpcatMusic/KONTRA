//! Library access: the keystream a library's own `.nicnt` describes, for its
//! encrypted presets and archive members. Keys are never logged or kept.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use ni_file::nis::LibraryKey;

/// Maximum `.nicnt` prefix scanned for `<JDX>` / `<HU>` registration tags.
const NICNT_SCAN_BYTES: u64 = 256 * 1024;

/// Parsed metadata and optional decryption keystream from a `.nicnt` file.
pub(crate) struct NicntInfo {
    pub snpid: String,
    pub name: Option<String>,
    pub keystream: Option<Arc<[u8]>>,
}

/// Reads `<SNPID>`, `<Name>`, and `<JDX>`/`<HU>` keystream from a `.nicnt` file.
pub(crate) fn read_nicnt(path: &Path) -> Option<NicntInfo> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(NICNT_SCAN_BYTES).read_to_end(&mut bytes))
        .ok()?;
    let snpid = text_tag(&bytes, b"SNPID")?;
    let name = text_tag(&bytes, b"Name");
    let keystream = field::<32>(&bytes, b"JDX")
        .zip(field::<16>(&bytes, b"HU"))
        .map(|(key, iv)| expand(key, iv));
    Some(NicntInfo {
        snpid,
        name,
        keystream,
    })
}

/// Wraps an already expanded 64 KiB keystream as an `ni_file::nis::LibraryKey`.
pub(crate) fn key_from_stream(stream: Arc<[u8]>) -> Arc<dyn LibraryKey> {
    Arc::new(Key(stream))
}

fn text_tag(bytes: &[u8], tag: &[u8]) -> Option<String> {
    let open = [b"<", tag, b">"].concat();
    let close = [b"</", tag, b">"].concat();
    let start = bytes.windows(open.len()).position(|w| w == open)? + open.len();
    let rest = bytes.get(start..)?;
    let end = rest.windows(close.len()).position(|w| w == close)?;
    let text = std::str::from_utf8(rest.get(..end)?.trim_ascii()).ok()?;
    (!text.is_empty()).then(|| text.to_owned())
}

/// The keystream of the library `path` belongs to, for presets.
pub(crate) fn library_key(path: &Path) -> Result<Arc<dyn LibraryKey>, String> {
    Ok(Arc::new(Key(keystream(path)?)))
}

/// The 64 KiB keystream of the library `path` belongs to; searches ancestors up
/// to the library root depth for a `.nicnt` carrying `<JDX>` and `<HU>` tags.
pub(crate) fn keystream(path: &Path) -> Result<Arc<[u8]>, String> {
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    let canon = path.canonicalize().ok();
    let primary = canon.as_deref().unwrap_or(&abs);
    match keystream_for(primary) {
        Ok(stream) => Ok(stream),
        Err(primary_err) => {
            if primary != abs
                && let Ok(stream) = keystream_for(&abs)
            {
                return Ok(stream);
            }
            Err(primary_err)
        }
    }
}

fn keystream_for(path: &Path) -> Result<Arc<[u8]>, String> {
    let mut saw_nicnt = None;
    for parent in path.ancestors().skip(1).take(crate::resources::DEPTH) {
        let Ok(read_dir) = std::fs::read_dir(parent) else {
            continue;
        };
        let mut files: Vec<_> = read_dir
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
                .and_then(|f| f.take(NICNT_SCAN_BYTES).read_to_end(&mut bytes))
                .map_err(|e| format!("reading {}: {e}", file.display()))?;
            if let (Some(key), Some(iv)) = (field::<32>(&bytes, b"JDX"), field::<16>(&bytes, b"HU"))
            {
                return Ok(expand(key, iv));
            }
            if saw_nicnt.is_none() {
                saw_nicnt = Some(file);
            }
        }
        // Stop climbing once we found a library `.nicnt` at this level.
        if saw_nicnt.is_some() {
            break;
        }
    }
    if let Some(nicnt) = saw_nicnt {
        Err(format!(
            "{} has no <JDX>/<HU> library access keys for {}",
            nicnt.display(),
            path.display()
        ))
    } else {
        Err(format!(
            "no .nicnt access data found for {}",
            path.display()
        ))
    }
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
        repeat(&self.0, 0, offset, bytes);
    }
}

/// Repeating XOR keystream starting at `origin`: file byte `origin + n` is
/// XORed with `stream[n % stream.len()]`. Deliberately not `Debug`.
struct RepeatXor {
    stream: Arc<[u8]>,
    origin: u64,
}

impl kontra_ir::ByteTransform for RepeatXor {
    fn apply(&self, offset: u64, bytes: &mut [u8]) {
        repeat(&self.stream, self.origin, offset, bytes);
    }
}

/// Wraps a repeating 64 KiB Kontakt keystream starting at `origin` for streaming reads.
pub(crate) fn repeat_keystream(stream: Arc<[u8]>, origin: u64) -> kontra_ir::Keystream {
    kontra_ir::Keystream::new(Arc::new(RepeatXor { stream, origin }))
}

fn repeat(stream: &[u8], origin: u64, at: u64, bytes: &mut [u8]) {
    let len = stream.len() as u64;
    if len == 0 {
        return;
    }
    // Whole runs of the key at a time, so the XOR vectorises.
    let mut position = (at.wrapping_sub(origin) % len) as usize;
    let mut rest = bytes;
    while !rest.is_empty() {
        let key = stream.get(position..).unwrap_or_default();
        let n = key.len().min(rest.len());
        let (head, tail) = std::mem::take(&mut rest).split_at_mut(n);
        head.iter_mut().zip(key).for_each(|(b, k)| *b ^= k);
        rest = tail;
        position = 0;
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

    #[test]
    fn finds_nicnt_even_when_nested_subfolder_has_samples_dir() {
        let root =
            std::env::temp_dir().join(format!("kontra-access-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("Instruments/4 Tonality");
        std::fs::create_dir_all(sub.join("Samples")).unwrap();
        let hex32 = "11".repeat(32);
        let hex16 = "22".repeat(16);
        std::fs::write(
            root.join("Lib.nicnt"),
            format!("<ProductHints><JDX>{hex32}</JDX><HU>{hex16}</HU></ProductHints>"),
        )
        .unwrap();
        let patch = sub.join("Patch.nki");
        std::fs::write(&patch, b"").unwrap();
        assert!(keystream(&patch).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn finds_nicnt_through_file_symlink_in_separate_scan_root() {
        let base =
            std::env::temp_dir().join(format!("kontra-access-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real_lib = base.join("RealLibrary");
        let scan_root = base.join("ScanRoot");
        std::fs::create_dir_all(real_lib.join("Instruments")).unwrap();
        std::fs::create_dir_all(&scan_root).unwrap();
        let hex32 = "aa".repeat(32);
        let hex16 = "bb".repeat(16);
        std::fs::write(
            real_lib.join("Real.nicnt"),
            format!("<ProductHints><JDX>{hex32}</JDX><HU>{hex16}</HU></ProductHints>"),
        )
        .unwrap();
        let real_patch = real_lib.join("Instruments/Patch.nki");
        std::fs::write(&real_patch, b"").unwrap();
        let alias = scan_root.join("Alias.nki");
        std::os::unix::fs::symlink(&real_patch, &alias).unwrap();

        assert!(keystream(&alias).is_ok());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
