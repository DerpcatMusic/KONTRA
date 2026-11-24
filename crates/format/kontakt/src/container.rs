//! From a file to Kontakt's chunk stream: NKS (FastLZ), NIS (maybe encrypted)
//! or a monolith holding one of them.

use std::io::{Cursor, Read, Seek};
use std::path::Path;

use kontra_format::{ProtectionStatus, ProtectionSystem};
use kontra_ir as ir;
use ni_file::NIFile;
use ni_file::file_container::NIFileContainer;
use ni_file::kontakt::KontaktChunks;
use ni_file::kontakt::objects::BPatchHeader;
use ni_file::nis::schema::{NISObject, PresetChunkItem, Repository};
use ni_file::nis::{AppSpecificProperties, ItemContainer, ItemType};

use crate::Error;

/// A preset larger than this is not a preset.
const MAX_PRESET: usize = 128 << 20;

/// The raw Kontakt chunk stream of a preset file (NKI, NKM, snapshot,
/// monolith), decompressed and decrypted.
///
/// # Errors
/// When the file is not a Kontakt container this reader understands.
pub fn chunks(path: &Path) -> Result<KontaktChunks, Error> {
    opened(path).map(|(chunks, _, _)| chunks)
}

/// Inspects whether `path` is open or requires unavailable Native Access
/// encryption keys, without decompressing or translating its full chunk stream.
///
/// # Errors
/// When the file cannot be opened or is not a recognized Kontakt container.
pub(crate) fn inspect_protection(path: &Path) -> Result<ProtectionStatus, Error> {
    let mut file = std::fs::File::open(path).map_err(|e| Error::new(path, "open", e))?;
    inspect_reader(&mut file, path, 0)
}

fn inspect_reader<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    depth: usize,
) -> Result<ProtectionStatus, Error> {
    if depth > 3 {
        return Err(Error::new(path, "container", "nested too deep"));
    }
    let container = NIFile::read(&mut *reader).map_err(|e| Error::new(path, "container", e))?;
    match container {
        NIFile::NKSContainer(nks) => match &nks.header {
            BPatchHeader::BPatchHeaderV42(_) => Ok(ProtectionStatus::Open),
            _ => Err(Error::new(
                path,
                "NKS preset",
                "legacy XML instruments are not read",
            )),
        },
        NIFile::NISoundContainer(nis) => inspect_nis_protection(nis, path, 0),
        NIFile::Monolith(container) => {
            let is_preset = |name: &str| {
                Path::new(name).extension().is_some_and(|e| {
                    ["nki", "nkm", "nkb"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                })
            };
            let mut presets = container.items.iter().filter(|i| is_preset(&i.filename));
            let (Some(preset), None) = (presets.next(), presets.next()) else {
                return Err(Error::new(
                    path,
                    "monolith",
                    "not exactly one preset member",
                ));
            };
            let bytes = container
                .read_item(reader, preset, MAX_PRESET as u64)
                .map_err(|e| Error::new(path, "monolith preset", e))?;
            inspect_reader(&mut Cursor::new(bytes), path, depth + 1)
        }
        _ => Err(Error::new(path, "container", "not an instrument")),
    }
}

fn inspect_nis_protection(
    container: ItemContainer,
    path: &Path,
    depth: usize,
) -> Result<ProtectionStatus, Error> {
    if depth > 3 {
        return Err(Error::new(path, "NIS", "nested too deep"));
    }
    if let Some(data) = container.find_data(&ItemType::AppSpecific) {
        let app =
            AppSpecificProperties::try_from(data).map_err(|e| Error::new(path, "NIS app", e))?;
        let inner = app
            .subtree_item
            .item()
            .map_err(|e| Error::new(path, "NIS subtree", e))?;
        return inspect_nis_protection(inner, path, depth + 1);
    }
    let NISObject::BNISoundPreset(preset) = Repository::from(container).infer_schema() else {
        return Err(Error::new(path, "NIS", "not a sound preset"));
    };
    let is_encrypted = preset
        .is_encrypted()
        .map_err(|e| Error::new(path, "NIS preset", e))?;
    if !is_encrypted {
        return Ok(ProtectionStatus::Open);
    }
    match crate::access::library_key(path) {
        Ok(_) => Ok(ProtectionStatus::Open),
        Err(reason) => Ok(ProtectionStatus::protected(
            ProtectionSystem::NativeAccess,
            format!("encrypted Kontakt preset requires .nicnt access data ({reason})"),
        )),
    }
}

/// [`chunks`], and the file's own monolith table when it is one: the file
/// opens once for both.
pub(crate) fn opened(path: &Path) -> Result<Opened<KontaktChunks>, Error> {
    let (bytes, monolith, encrypted) = decoded(path)?;
    let chunks = KontaktChunks::read(Cursor::new(bytes))
        .map_err(|e| Error::new(path, "Kontakt chunks", e))?;
    Ok((chunks, monolith, encrypted))
}

/// What a file opens to: its chunks (or their bytes), its own monolith
/// table when it is one, and whether it was encrypted.
pub(crate) type Opened<T> = (T, Option<NIFileContainer>, Option<bool>);

/// The chunk stream's bytes, the monolith table and whether it was encrypted.
pub(crate) fn decoded(path: &Path) -> Result<Opened<Vec<u8>>, Error> {
    let mut file = std::fs::File::open(path).map_err(|e| Error::new(path, "open", e))?;
    let mut monolith = None;
    let mut encrypted = None;
    let bytes = payload(&mut file, path, 0, &mut monolith, &mut encrypted)?;
    Ok((bytes, monolith, encrypted))
}

/// The instrument of an already decoded chunk stream, as `path` names it
/// (its samples resolve beside that file).
pub(crate) fn from_bytes(
    path: &Path,
    bytes: Vec<u8>,
    encrypted: Option<bool>,
) -> Result<ir::Instrument, Error> {
    let chunks = KontaktChunks::read(Cursor::new(bytes))
        .map_err(|e| Error::new(path, "Kontakt chunks", e))?;
    crate::translate::instrument(path, &chunks, None, &[], encrypted, None)
}

fn payload<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    depth: usize,
    monolith: &mut Option<NIFileContainer>,
    encrypted: &mut Option<bool>,
) -> Result<Vec<u8>, Error> {
    if depth > 3 {
        return Err(Error::new(path, "container", "nested too deep"));
    }
    let container = NIFile::read(&mut *reader).map_err(|e| Error::new(path, "container", e))?;
    match container {
        NIFile::NKSContainer(nks) => match &nks.header {
            BPatchHeader::BPatchHeaderV42(h) => {
                let expected = h.decompressed_length as usize;
                if expected > MAX_PRESET {
                    return Err(Error::new(path, "NKS preset", "too large"));
                }
                kontra_fastlz::expand(&nks.compressed_data, expected)
                    .map_err(|e| Error::new(path, "NKS preset", e))
            }
            _ => Err(Error::new(
                path,
                "NKS preset",
                "legacy XML instruments are not read",
            )),
        },
        NIFile::NISoundContainer(nis) => nis_payload(nis, path, 0, encrypted),
        NIFile::Monolith(container) => {
            let is_preset = |name: &str| {
                Path::new(name).extension().is_some_and(|e| {
                    ["nki", "nkm", "nkb"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                })
            };
            let mut presets = container.items.iter().filter(|i| is_preset(&i.filename));
            let (Some(preset), None) = (presets.next(), presets.next()) else {
                return Err(Error::new(
                    path,
                    "monolith",
                    "not exactly one preset member",
                ));
            };
            let bytes = container
                .read_item(reader, preset, MAX_PRESET as u64)
                .map_err(|e| Error::new(path, "monolith preset", e))?;
            if depth == 0 {
                *monolith = Some(container);
            }
            payload(
                &mut Cursor::new(bytes),
                path,
                depth + 1,
                &mut None,
                encrypted,
            )
        }
        _ => Err(Error::new(path, "container", "not an instrument")),
    }
}

fn nis_payload(
    container: ItemContainer,
    path: &Path,
    depth: usize,
    encrypted: &mut Option<bool>,
) -> Result<Vec<u8>, Error> {
    if depth > 3 {
        return Err(Error::new(path, "NIS", "nested too deep"));
    }
    if let Some(data) = container.find_data(&ItemType::AppSpecific) {
        let app =
            AppSpecificProperties::try_from(data).map_err(|e| Error::new(path, "NIS app", e))?;
        let inner = app
            .subtree_item
            .item()
            .map_err(|e| Error::new(path, "NIS subtree", e))?;
        return nis_payload(inner, path, depth + 1, encrypted);
    }
    let NISObject::BNISoundPreset(preset) = Repository::from(container).infer_schema() else {
        return Err(Error::new(path, "NIS", "not a sound preset"));
    };
    let is_encrypted = preset
        .is_encrypted()
        .map_err(|e| Error::new(path, "NIS preset", e))?;
    *encrypted = Some(is_encrypted);
    let key = if is_encrypted {
        Some(crate::access::library_key(path).map_err(|e| {
            Error::protected(
                path,
                "library access",
                ProtectionStatus::protected(
                    ProtectionSystem::NativeAccess,
                    format!("encrypted Kontakt preset requires .nicnt access data ({e})"),
                ),
            )
        })?)
    } else {
        None
    };
    let item = preset
        .encryption_item_with_key(key.as_deref())
        .map_err(|e| Error::new(path, "NIS preset", e))?;
    let subtree = item
        .subtree
        .item()
        .map_err(|e| Error::new(path, "NIS preset", e))?;
    let chunk = PresetChunkItem::from(subtree)
        .properties()
        .map_err(|e| Error::new(path, "NIS preset chunk", e))?;
    Ok(chunk.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structured(
        id: u16,
        version: u16,
        public: &[u8],
        private: &[u8],
        children: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.push(1); // structured flag
        body.extend_from_slice(&version.to_le_bytes());
        body.extend_from_slice(&(private.len() as u32).to_le_bytes());
        body.extend_from_slice(private);
        body.extend_from_slice(&(public.len() as u32).to_le_bytes());
        body.extend_from_slice(public);
        let child_bytes: Vec<u8> = children.iter().flatten().copied().collect();
        body.extend_from_slice(&(child_bytes.len() as u32).to_le_bytes());
        body.extend_from_slice(&child_bytes);

        let mut chunk = Vec::new();
        chunk.extend_from_slice(&id.to_le_bytes());
        chunk.extend_from_slice(&(body.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&body);
        chunk
    }

    fn utf16_len_str(s: &str) -> Vec<u8> {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut out = (units.len() as u32).to_le_bytes().to_vec();
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    /// Wraps a raw Kontakt chunk stream in a minimal uncompressed FastLZ NKS v4.2 container.
    fn nks_v42(chunk_stream: &[u8]) -> Vec<u8> {
        // FastLZ level 1 literal runs (max 32 bytes per run, header byte = len - 1).
        let mut compressed = Vec::new();
        for piece in chunk_stream.chunks(32) {
            compressed.push((piece.len() - 1) as u8);
            compressed.extend_from_slice(piece);
        }
        let mut out = Vec::new();
        // NKS container header: magic (4), compressed_len (4), header_version (2 > 271 => v42)
        out.extend_from_slice(&0x7FA8_9012u32.to_le_bytes()); // Kontakt42LittleEndian
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&272u16.to_le_bytes());
        // BPatchHeaderV42 (212 bytes)
        let mut hdr = vec![0u8; 212];
        hdr[0..4].copy_from_slice(&0xEA37_631Au32.to_le_bytes()); // v42 header magic
        hdr[4..6].copy_from_slice(&0u16.to_le_bytes()); // patch type NKI
        hdr[10..14].copy_from_slice(b"KtN4"); // app signature
        hdr[14..18].copy_from_slice(&1_700_000_000u32.to_le_bytes()); // unix timestamp
        hdr[176..180].copy_from_slice(&(chunk_stream.len() as u32).to_le_bytes()); // decompressed_length (followed by 32 padding bytes)
        out.extend_from_slice(&hdr);
        out.extend_from_slice(&compressed);
        // BPatchMetaInfoHeader footer: magic (4), u_a (1), u_b (1), u_c (2)
        out.extend_from_slice(&0xB00E_E1AEu32.to_le_bytes());
        out.extend_from_slice(&[1, 1, 0, 0]);
        out
    }

    /// Wraps members in a modern NI FileContainer monolith (intentionally sharing
    /// duplicate `index = 0` across items to verify `read_item` handling).
    fn file_container(members: &[(&str, &[u8])]) -> Vec<u8> {
        let total: u64 = members.iter().map(|(_, d)| d.len() as u64).sum();
        let mut out = b"/\\ NI FC MTD  /\\".to_vec();
        out.extend_from_slice(&[0u8; 256]);
        out.extend_from_slice(&(members.len() as u64).to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(b"/\\ NI FC TOC  /\\");
        out.extend_from_slice(&[0u8; 600]);
        let mut end = 0u64;
        for (name, data) in members {
            end += data.len() as u64;
            out.extend_from_slice(&0u64.to_le_bytes()); // duplicate index = 0
            out.extend_from_slice(&[0u8; 16]);
            let mut fname: Vec<u8> = name
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            fname.resize(600, 0);
            out.extend_from_slice(&fname);
            out.extend_from_slice(&0u64.to_le_bytes());
            out.extend_from_slice(&end.to_le_bytes());
        }
        out.extend_from_slice(&0xF1F1_F1F1_F1F1_F1F1u64.to_le_bytes());
        out.extend_from_slice(&[0u8; 16]);
        out.extend_from_slice(b"/\\ NI FC TOC  /\\");
        out.extend_from_slice(&[0u8; 592]);
        for (_, data) in members {
            out.extend_from_slice(data);
        }
        out
    }

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(label: &str) -> Self {
            static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "kontra_container_test_{label}_{}_{}",
                std::process::id(),
                id
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn authored_file_container_monolith_resolves_preset_and_embedded_sample() {
        // Minimal valid Kontakt program (0x28 v0xa8) with 1 empty group and 1 zone referencing mono.wav.
        let mut prog_pub = utf16_len_str("MonoTest");
        prog_pub.extend_from_slice(&0u32.to_le_bytes()); // num_bytes_samples_total
        prog_pub.extend_from_slice(&[0u8; 256]); // padded program public tail

        let mut grp_pub = utf16_len_str("G0");
        grp_pub.extend_from_slice(&1.0f32.to_le_bytes()); // volume
        grp_pub.extend_from_slice(&0.0f32.to_le_bytes()); // pan
        grp_pub.extend_from_slice(&1.0f32.to_le_bytes()); // tune
        grp_pub.extend_from_slice(&[1, 0, 0, 0, 0]);
        grp_pub.extend_from_slice(&[0u8; 96]);
        let group_chunk = structured(0x04, 0x74, &grp_pub, &[], &[]);

        // GroupList (0x33): u32 count = 1, then group chunk
        let mut gl_body = 1u32.to_le_bytes().to_vec();
        gl_body.extend_from_slice(&group_chunk);
        let mut group_list = 0x33u16.to_le_bytes().to_vec();
        group_list.extend_from_slice(&(gl_body.len() as u32).to_le_bytes());
        group_list.extend_from_slice(&gl_body);

        // Empty ZoneList (0x34): u32 count = 0
        let mut zone_list = 0x34u16.to_le_bytes().to_vec();
        zone_list.extend_from_slice(&4u32.to_le_bytes());
        zone_list.extend_from_slice(&0u32.to_le_bytes());

        let program = structured(0x28, 0xa8, &prog_pub, &[], &[group_list, zone_list]);
        let nki_bytes = nks_v42(&program);
        let wav_bytes = b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x80\xbb\x00\x00\x00\x77\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00";
        let monolith =
            file_container(&[("Patch.nki", &nki_bytes), ("Samples|mono.wav", wav_bytes)]);

        let dir = TempDir::new("mono");
        let path = dir.0.join("Bundle.nki");
        std::fs::write(&path, &monolith).unwrap();

        assert_eq!(inspect_protection(&path).unwrap(), ProtectionStatus::Open);
        let (chunks, table, encrypted) = opened(&path).unwrap();
        assert_eq!(encrypted, None);
        assert!(chunks.find_first(0x28).is_some());
        let table = table.expect("monolith table preserved");
        assert_eq!(table.items.len(), 2);

        // Verify monolith sample resolution via Resolver.
        let mut resolver = crate::samples::Resolver::new(&path, Some(table)).unwrap();
        let loc = resolver
            .resolve("Samples/mono.wav")
            .unwrap()
            .expect("embedded sample resolved");
        assert_eq!(loc.size, Some(wav_bytes.len() as u64));
        assert!(loc.offset > 0);
    }

    #[test]
    fn legacy_nks_v1_monolith_returns_unsupported_error_without_panicking() {
        // Legacy NKS v1 header (magic 0x5E70AC54 / Kontakt1LittleEndian 0xB36EE55E).
        let mut legacy = Vec::new();
        legacy.extend_from_slice(&0xB36E_E55Eu32.to_le_bytes()); // Kontakt1LittleEndian
        legacy.extend_from_slice(&0u32.to_le_bytes());
        legacy.extend_from_slice(&0u16.to_le_bytes());
        legacy.extend_from_slice(&[0u8; 32]); // BPatchHeaderV1 body
        let monolith = file_container(&[("Legacy.nki", &legacy)]);

        let dir = TempDir::new("legacy");
        let path = dir.0.join("LegacyMono.nki");
        std::fs::write(&path, &monolith).unwrap();

        let err = opened(&path).unwrap_err();
        assert!(
            err.reason.contains("legacy XML") || err.what == "container",
            "unexpected error: {err}"
        );
        assert!(inspect_protection(&path).is_err());
    }
}
