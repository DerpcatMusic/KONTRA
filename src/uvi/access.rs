//! Automatic local UFS content preparation. No account state, reader constants, or bank keys are bundled.
use super::{
    crypto,
    ufs::{Directory, Member, Ufs},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
pub(crate) struct ContentState {
    pub(crate) key: u64,
    #[serde(default)]
    pub(crate) bank: Option<String>,
}

impl ContentState {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let bytes = read_private(path)?;
        if bytes.len() == 8 {
            Ok(Self {
                key: u64::from_le_bytes(bytes.try_into().unwrap()),
                bank: None,
            })
        } else {
            serde_json::from_slice(&bytes).context("Invalid local content-state file")
        }
    }
}

/// Verify all PNG CRCs and critical chunk structure without decoding IDAT pixels.
pub(crate) fn validate_png(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "Invalid PNG signature"
    );
    let mut offset = 8usize;
    let (mut chunks, mut data, mut data_ended) = (0usize, false, false);
    let (mut depth, mut color, mut palette) = (0u8, 0u8, 0usize);
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 8)
            .context("Truncated PNG chunk")?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        ensure!(size <= i32::MAX as usize, "Invalid PNG chunk length");
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(size))
            .context("PNG chunk overflow")?;
        let chunk = bytes
            .get(offset + 4..end)
            .context("Truncated PNG chunk data")?;
        let crc = u32::from_be_bytes(chunk[chunk.len() - 4..].try_into().unwrap());
        ensure!(
            crc32fast::hash(&chunk[..chunk.len() - 4]) == crc,
            "PNG CRC mismatch"
        );
        let kind = &header[4..8];
        ensure!(
            kind.iter().all(u8::is_ascii_alphabetic),
            "Invalid PNG chunk type"
        );
        if chunks == 0 {
            ensure!(kind == b"IHDR" && size == 13, "PNG must start with IHDR");
            let ihdr = &chunk[4..17];
            ensure!(
                (1..=i32::MAX as u32)
                    .contains(&u32::from_be_bytes(ihdr[..4].try_into().unwrap()))
                    && (1..=i32::MAX as u32)
                        .contains(&u32::from_be_bytes(ihdr[4..8].try_into().unwrap())),
                "Invalid PNG dimensions"
            );
            ensure!(
                match ihdr[9] {
                    0 => matches!(ihdr[8], 1 | 2 | 4 | 8 | 16),
                    2 | 4 | 6 => matches!(ihdr[8], 8 | 16),
                    3 => matches!(ihdr[8], 1 | 2 | 4 | 8),
                    _ => false,
                },
                "Invalid PNG pixel format"
            );
            ensure!(
                ihdr[10] == 0 && ihdr[11] == 0 && ihdr[12] <= 1,
                "Invalid PNG encoding"
            );
            (depth, color) = (ihdr[8], ihdr[9]);
        } else {
            match kind {
                b"IHDR" => anyhow::bail!("Repeated PNG IHDR"),
                b"PLTE" => {
                    ensure!(
                        palette == 0 && !data && matches!(color, 2 | 3 | 6),
                        "Invalid PNG palette order or color type"
                    );
                    ensure!(
                        (3..=768).contains(&size) && size.is_multiple_of(3),
                        "Invalid PNG palette size"
                    );
                    palette = size / 3;
                    ensure!(
                        color != 3 || palette <= 1usize << depth,
                        "PNG palette exceeds bit depth"
                    );
                }
                b"IDAT" => {
                    ensure!(!data_ended, "Nonconsecutive PNG IDAT chunks");
                    ensure!(color != 3 || palette > 0, "Indexed PNG lacks palette");
                    data = true;
                }
                b"IEND" => {}
                _ => ensure!(kind[0] & 0x20 != 0, "Unknown critical PNG chunk"),
            }
        }
        data_ended |= data && kind != b"IDAT";
        chunks += 1;
        offset = end;
        if kind == b"IEND" {
            ensure!(
                size == 0 && data && offset == bytes.len(),
                "Invalid PNG end"
            );
            return Ok(chunks);
        }
    }
    anyhow::bail!("PNG has no IEND")
}

#[derive(Serialize, Deserialize)]
struct StoredState {
    version: u8,
    key: u64,
    bank: String,
    uuid: [u8; 16],
    physical_size: u64,
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        std::fs::symlink_metadata(path)?.is_file(),
        "Private UVI access path is not a regular file"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            file.metadata()?.permissions().mode() & 0o077 == 0,
            "Content-state file must be private (chmod 600)"
        );
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "Content-state file exceeds limit");
    Ok(bytes)
}

fn candidates(directory: &Directory) -> Vec<&Member> {
    let mut found: Vec<_> = directory
        .files
        .iter()
        .filter(|m| {
            m.mode == 2
                && (45..=16 << 20).contains(&m.size)
                && m.name.to_ascii_lowercase().ends_with(".png")
        })
        .collect();
    found.sort_by_key(|m| m.size);
    found.truncate(8);
    found
}

fn verify_key(bank: &Ufs, directory: &Directory, key: u64) -> Result<()> {
    for member in candidates(directory) {
        if let Ok(bytes) = bank.read_member(member, directory.metadata_key, Some(key))
            && validate_png(&bytes).is_ok()
        {
            return Ok(());
        }
    }
    anyhow::bail!("UVI content state did not pass complete PNG CRC verification")
}

/// Recover only from the verified v3 cipher and standard PNG header; verify every CRC.
/// At most eight candidate members, each bounded to 16 MiB, are examined.
pub(crate) fn recover_content_state(
    path: &Path,
    bank: &Ufs,
    directory: &Directory,
) -> Result<ContentState> {
    let found = candidates(directory);
    ensure!(
        !found.is_empty(),
        "This UVI bank has no supported encrypted PNG for automatic local setup"
    );
    let mut file = File::open(path)?;
    ensure!(
        file.metadata()?.len() == bank.header.physical_size,
        "UVI bank changed during local setup"
    );
    for member in found {
        ensure!(
            member
                .offset
                .checked_add(member.size)
                .is_some_and(|end| end <= bank.header.physical_size),
            "UVI PNG range exceeds container"
        );
        file.seek(SeekFrom::Start(member.offset))?;
        let mut cipher = [0; 16];
        file.read_exact(&mut cipher)?;
        let plain = *b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        if let Ok(key) = crypto::recover_key(&cipher, &plain, member.offset)
            && let Ok(bytes) = bank.read_member(member, directory.metadata_key, Some(key))
            && validate_png(&bytes).is_ok()
        {
            return Ok(ContentState {
                key,
                bank: Some(bank.header.bank_name.clone()),
            });
        }
    }
    anyhow::bail!("This UVI bank could not pass automatic local content verification")
}

fn private_directory(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(metadata.is_dir(), "UVI access store is not a directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "UVI access store must be private (chmod 700)"
        );
    }
    Ok(())
}

/// Publish a complete, synced owner-only record atomically. Existing CLI outputs are preserved.
pub(crate) fn save_content_state(
    path: &Path,
    bank: &Ufs,
    state: &ContentState,
    replace: bool,
) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let record = StoredState {
        version: 1,
        key: state.key,
        bank: bank.header.bank_name.clone(),
        uuid: bank.header.uuid,
        physical_size: bank.header.physical_size,
    };
    let bytes = serde_json::to_vec(&record)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temporary = None;
    for _ in 0..16 {
        let path = parent.join(format!(
            ".uvi-access-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Relaxed)
        ));
        match options.open(&path) {
            Ok(file) => {
                temporary = Some((path, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let (temporary, mut file) =
        temporary.context("Could not reserve private UVI access temporary file")?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if replace {
            std::fs::rename(&temporary, path)?;
        } else {
            std::fs::hard_link(&temporary, path)?;
        }
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temporary);
    result
}

/// First supported load prepares bank access locally; later loads use the UUID-bound private record.
/// Legacy explicit states are accepted only after full member verification, then upgraded atomically.
pub(crate) fn ensure_content_state(
    path: &Path,
    bank: &Ufs,
    directory: &Directory,
    store: &Path,
) -> Result<Option<ContentState>> {
    if !directory.files.iter().any(|m| m.mode == 2) {
        return Ok(None);
    }
    private_directory(store)?;
    let filename = bank
        .header
        .uuid
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + ".json";
    let output = store.join(filename);
    match read_private(&output) {
        Ok(bytes) => {
            if serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .is_some_and(|v| v.get("version").is_some())
            {
                let record: StoredState = serde_json::from_slice(&bytes)
                    .context("Invalid UUID-bound UVI access record")?;
                ensure!(record.version == 1, "Unsupported UVI access record version");
                if record.uuid == bank.header.uuid
                    && record.physical_size == bank.header.physical_size
                    && record.bank == bank.header.bank_name
                {
                    verify_key(bank, directory, record.key)?;
                    return Ok(Some(ContentState {
                        key: record.key,
                        bank: Some(bank.header.bank_name.clone()),
                    }));
                }
                let state = recover_content_state(path, bank, directory)?;
                save_content_state(&output, bank, &state, true)?;
                return Ok(Some(state));
            }
            let legacy = ContentState::open(&output)?;
            let identity_matches = legacy.bank.as_ref().is_none_or(|identity| {
                identity == &bank.header.bank_name
                    || std::fs::canonicalize(identity).ok() == std::fs::canonicalize(path).ok()
            });
            let state = if identity_matches {
                verify_key(bank, directory, legacy.key)?;
                ContentState {
                    key: legacy.key,
                    bank: Some(bank.header.bank_name.clone()),
                }
            } else {
                recover_content_state(path, bank, directory)?
            };
            save_content_state(&output, bank, &state, true)?;
            Ok(Some(state))
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            let state = recover_content_state(path, bank, directory)?;
            match save_content_state(&output, bank, &state, false) {
                Ok(()) => Ok(Some(state)),
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
                {
                    // Another loader published first; validate its bounded record instead.
                    let record: StoredState = serde_json::from_slice(&read_private(&output)?)?;
                    ensure!(
                        record.version == 1
                            && record.uuid == bank.header.uuid
                            && record.physical_size == bank.header.physical_size
                            && record.bank == bank.header.bank_name
                            && record.key == state.key,
                        "Concurrent UVI access record differs from verified bank"
                    );
                    Ok(Some(state))
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

/// Bounded installed-reader discovery; configured paths and environment overrides remain authoritative.
pub(crate) fn reader_path(configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = configured {
        return Ok(path.to_owned());
    }
    if let Some(path) = std::env::var_os("KONTRA_UVI_READER") {
        return Ok(path.into());
    }
    let relative = Path::new("drive_c/Program Files/UVI Workstation/UVIWorkstationx64.exe");
    let mut candidates = Vec::new();
    if let Some(prefix) = std::env::var_os("WINEPREFIX") {
        candidates.push(PathBuf::from(prefix).join(relative));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".wine").join(relative));
    }
    for variable in ["PROGRAMFILES", "ProgramW6432"] {
        if let Some(path) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(path).join("UVI Workstation/UVIWorkstationx64.exe"));
        }
    }
    candidates.extend(
        [
            "/mnt/c/Program Files/UVI Workstation/UVIWorkstationx64.exe",
            "/mnt/Windows11/Program Files/UVI Workstation/UVIWorkstationx64.exe",
        ]
        .map(PathBuf::from),
    );
    candidates.into_iter().find(|path| path.is_file()).context(
        "No local UVI reader was found; select a verified UVI Workstation reader in settings",
    )
}

/// JSON type errors can echo private field values; journal only their category.
pub(crate) fn failure_reason(error: &anyhow::Error) -> String {
    if error.chain().any(|cause| cause.is::<serde_json::Error>()) {
        "Invalid private UVI access record".into()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_failure_reason_does_not_echo_private_json_values() {
        let error = serde_json::from_slice::<ContentState>(br#"{"key":"private-value-marker"}"#)
            .err().unwrap();
        let error = anyhow::Error::new(error).context("Invalid local content-state file");
        assert_eq!(failure_reason(&error), "Invalid private UVI access record");
        let error = anyhow::anyhow!("Content-state file must be private (chmod 600)");
        assert_eq!(failure_reason(&error), error.to_string());
    }

    fn authored_png() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let start = png.len();
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            let crc = crc32fast::hash(&png[start..]);
            png.extend_from_slice(&crc.to_be_bytes());
        };
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
        chunk(b"IDAT", &[0x78, 1, 1, 2, 0, 0xfd, 0xff, 0, 0, 0, 2, 0, 1]);
        chunk(b"IEND", &[]);
        png
    }

    #[test]
    fn automatic_private_setup_binds_identity_and_upgrades_verified_legacy_state() {
        let root =
            std::env::temp_dir().join(format!("kontra-authored-access-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("authored.ufs");
        let store = root.join("private");
        let plain = authored_png();
        let mixed = 328u64.wrapping_mul(0xc6a4_a793_5bd1_e995);
        let key = (mixed ^ (mixed >> 47)).wrapping_mul(0xc6a4_a793_5bd1_e995);
        // Authored zero initial stream state keeps debug recovery bounded to task 0.
        let mut encrypted = plain.clone();
        crypto::transform_blocks(&mut encrypted, key, 328);
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[8..24].fill(1);
        bytes[48..57].copy_from_slice(b"Synthetic");
        bytes.extend_from_slice(&(encrypted.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&encrypted);
        let size = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&size.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let bank = Ufs::open(&path).unwrap();
        let mut directory = Directory {
            files: vec![Member {
                record_offset: 999,
                name: "authored.png".into(),
                path: Some("authored.png".into()),
                parent: None,
                size: plain.len() as u64,
                offset: 328,
                mode: 2,
                footer: Vec::new(),
            }],
            directories: Vec::new(),
            records: Vec::new(),
            warnings: Vec::new(),
            metadata_key: 0,
        };
        let state = ensure_content_state(&path, &bank, &directory, &store)
            .unwrap()
            .unwrap();
        assert_eq!(state.key, key);
        let output = store.join("01010101010101010101010101010101.json");
        let stored = read_private(&output).unwrap();
        let record: StoredState = serde_json::from_slice(&stored).unwrap();
        assert_eq!(record.uuid, bank.header.uuid);
        assert_eq!(record.physical_size, size);
        assert_eq!(ContentState::open(&output).unwrap().key, key);
        assert_eq!(
            ensure_content_state(&path, &bank, &directory, &store)
                .unwrap()
                .unwrap()
                .key,
            key
        );
        let mut corrupt_record = serde_json::from_slice::<serde_json::Value>(&stored).unwrap();
        corrupt_record["key"] = (key ^ 1).into();
        let corrupt_record = serde_json::to_vec(&corrupt_record).unwrap();
        std::fs::write(&output, &corrupt_record).unwrap();
        assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
        assert_eq!(read_private(&output).unwrap(), corrupt_record);
        std::fs::write(&output, &stored).unwrap();
        assert!(save_content_state(&output, &bank, &state, false).is_err());
        assert_eq!(read_private(&output).unwrap(), stored);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&store).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut wrong = serde_json::from_slice::<serde_json::Value>(&stored).unwrap();
        wrong["physical_size"] = (size + 1).into();
        std::fs::write(&output, serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert_eq!(
            ensure_content_state(&path, &bank, &directory, &store)
                .unwrap()
                .unwrap()
                .key,
            key
        );
        assert_eq!(
            serde_json::from_slice::<StoredState>(&read_private(&output).unwrap())
                .unwrap()
                .physical_size,
            size
        );
        // The same bank UUID can receive an updated physical container.
        bytes.extend_from_slice(&[0; 8]);
        let new_size = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&new_size.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let bank = Ufs::open(&path).unwrap();
        assert_eq!(
            ensure_content_state(&path, &bank, &directory, &store)
                .unwrap()
                .unwrap()
                .key,
            key
        );
        assert_eq!(
            serde_json::from_slice::<StoredState>(&read_private(&output).unwrap())
                .unwrap()
                .physical_size,
            new_size
        );
        std::fs::write(&output, b"{broken").unwrap();
        assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
        assert_eq!(read_private(&output).unwrap(), b"{broken");
        std::fs::write(&output, key.to_le_bytes()).unwrap();
        ensure_content_state(&path, &bank, &directory, &store).unwrap();
        assert_eq!(
            serde_json::from_slice::<StoredState>(&read_private(&output).unwrap())
                .unwrap()
                .version,
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
            let linked_store = root.join("linked-store");
            let before_link = read_private(&output).unwrap();
            symlink(&store, &linked_store).unwrap();
            assert!(ensure_content_state(&path, &bank, &directory, &linked_store).is_err());
            assert_eq!(read_private(&output).unwrap(), before_link);
            std::fs::remove_file(linked_store).unwrap();
            let outside = root.join("unrelated.json");
            std::fs::write(&outside, b"authored unrelated data").unwrap();
            std::fs::remove_file(&output).unwrap();
            symlink(&outside, &output).unwrap();
            assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"authored unrelated data");
            std::fs::remove_file(&output).unwrap();
            save_content_state(&output, &bank, &state, false).unwrap();
        }
        // A broken member cannot validate or replace a legacy state.
        std::fs::write(&output, key.to_le_bytes()).unwrap();
        bytes[328 + 45] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
        assert_eq!(read_private(&output).unwrap(), key.to_le_bytes());
        std::fs::remove_file(&output).unwrap();
        directory.files[0].name = "unsupported.bin".into();
        assert!(ensure_content_state(&path, &bank, &directory, &store).is_err());
        assert!(!output.exists());
        directory.files[0].mode = 0;
        assert!(
            ensure_content_state(&path, &bank, &directory, &root.join("unused"))
                .unwrap()
                .is_none()
        );
        assert!(!root.join("unused").exists());
        assert!(std::fs::read_dir(&store).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        assert_eq!(
            reader_path(Some(Path::new("configured-reader"))).unwrap(),
            Path::new("configured-reader")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn complete_png_requires_all_crcs_and_end_marker() {
        let png = authored_png();
        assert_eq!(validate_png(&png).unwrap(), 3);
        let mut corrupt = png.clone();
        corrupt[45] ^= 1;
        assert!(validate_png(&corrupt).is_err());
        assert!(validate_png(&png[..png.len() - 1]).is_err());
        let mut extra = png;
        extra.push(0);
        assert!(validate_png(&extra).is_err());
    }
    #[test]
    fn complete_png_rejects_crc_correct_invalid_pixel_formats() {
        for (depth, color) in [(7, 0), (8, 1), (4, 2), (16, 3), (1, 4), (4, 6)] {
            let mut png = authored_png();
            png[24] = depth;
            png[25] = color;
            let crc = crc32fast::hash(&png[12..29]);
            png[29..33].copy_from_slice(&crc.to_be_bytes());
            assert!(
                validate_png(&png).is_err(),
                "Accepted depth {depth}, color {color}"
            );
        }
    }
    #[test]
    fn complete_png_requires_critical_chunk_structure() {
        fn png(depth: u8, color: u8, chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
            let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut append = |kind: &[u8; 4], data: &[u8]| {
                png.extend_from_slice(&(data.len() as u32).to_be_bytes());
                let start = png.len();
                png.extend_from_slice(kind);
                png.extend_from_slice(data);
                let crc = crc32fast::hash(&png[start..]);
                png.extend_from_slice(&crc.to_be_bytes());
            };
            append(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, depth, color, 0, 0, 0]);
            for &(kind, data) in chunks {
                append(kind, data);
            }
            png
        }
        let palette = [0; 3];
        for chunks in [
            vec![(b"IDAT", &[][..]), (b"IEND", &[][..])],
            vec![
                (b"PLTE", &palette[..]),
                (b"PLTE", &palette[..]),
                (b"IDAT", &[][..]),
                (b"IEND", &[][..]),
            ],
            vec![
                (b"IDAT", &[][..]),
                (b"PLTE", &palette[..]),
                (b"IEND", &[][..]),
            ],
            vec![(b"PLTE", &[][..]), (b"IDAT", &[][..]), (b"IEND", &[][..])],
            vec![
                (b"PLTE", &[0; 4][..]),
                (b"IDAT", &[][..]),
                (b"IEND", &[][..]),
            ],
            vec![
                (b"PLTE", &[0; 771][..]),
                (b"IDAT", &[][..]),
                (b"IEND", &[][..]),
            ],
        ] {
            assert!(validate_png(&png(8, 3, &chunks)).is_err());
        }
        assert!(
            validate_png(&png(
                1,
                3,
                &[(b"PLTE", &[0; 9]), (b"IDAT", &[]), (b"IEND", &[])]
            ))
            .is_err()
        );
        for color in [0, 4] {
            assert!(
                validate_png(&png(
                    8,
                    color,
                    &[(b"PLTE", &palette), (b"IDAT", &[]), (b"IEND", &[])]
                ))
                .is_err()
            );
        }
        for kind in [b"IHDR", b"ABCD", b"x1XX"] {
            assert!(validate_png(&png(8, 0, &[(kind, &[]), (b"IDAT", &[]), (b"IEND", &[])])).is_err());
        }
        assert!(
            validate_png(&png(
                8,
                0,
                &[
                    (b"IDAT", &[]),
                    (b"tEXt", &[]),
                    (b"IDAT", &[]),
                    (b"IEND", &[])
                ]
            ))
            .is_err()
        );
        assert!(validate_png(&png(8, 0, &[(b"IEND", &[])])).is_err());
        assert!(validate_png(&png(8, 0, &[(b"IDAT", &[]), (b"IEND", &[0])])).is_err());
        assert!(
            validate_png(&png(
                8,
                0,
                &[(b"IDAT", &[]), (b"IEND", &[]), (b"IEND", &[])]
            ))
            .is_err()
        );
        for (depth, color) in [(1, 3), (8, 2), (16, 6)] {
            assert_eq!(
                validate_png(&png(
                    depth,
                    color,
                    &[
                        (b"PLTE", &palette),
                        (b"IDAT", &[]),
                        (b"IDAT", &[]),
                        (b"IEND", &[])
                    ]
                ))
                .unwrap(),
                5
            );
        }
        // Unknown ancillary and future reserved-bit chunks are ignored by decoder policy.
        assert_eq!(
            validate_png(&png(
                8,
                0,
                &[
                    (b"zzzz", &[]),
                    (b"IDAT", &[]),
                    (b"IDAT", &[]),
                    (b"tEXt", &[]),
                    (b"IEND", &[])
                ]
            ))
            .unwrap(),
            6
        );
        let mut oversized = png(8, 0, &[(b"IDAT", &[]), (b"IEND", &[])]);
        oversized[16..20].copy_from_slice(&0x8000_0000u32.to_be_bytes());
        let crc = crc32fast::hash(&oversized[12..29]);
        oversized[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(validate_png(&oversized).is_err());
    }
}
